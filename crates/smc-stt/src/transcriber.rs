use crate::error::{SttError, SttResult};
use crate::mel::{MelFilterbank, N_MELS, WHISPER_FRAMES};
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokenizers::Tokenizer;
use tracing::info;

pub const DEFAULT_IDLE_UNLOAD_SECS: u64 = 300; // 5 minutes

// Whisper special token IDs for English transcription without timestamps
pub const SOT_TOKEN: i64 = 50258; // <|startoftranscript|>
pub const EN_TOKEN: i64 = 50259; // <|en|>
pub const TRANSCRIBE_TOKEN: i64 = 50359; // <|transcribe|>
pub const NO_TIMESTAMPS_TOKEN: i64 = 50363; // <|notimestamps|>
pub const EOT_TOKEN: i64 = 50257; // <|endoftranscript|>

pub trait Transcriber: Send + Sync {
    /// Transcribes 16kHz mono audio slice to natural language text.
    fn transcribe(&self, audio: &[f32]) -> SttResult<String>;

    /// Returns whether the ONNX models are currently resident in RAM.
    fn is_loaded(&self) -> bool;

    /// Forces model unload to free RAM.
    fn unload(&self);

    /// Checks if the model has been idle longer than the timeout and unloads if needed.
    fn maybe_unload_idle(&self) -> bool;
}

#[derive(Debug, Clone)]
pub struct WhisperConfig {
    pub encoder_path: PathBuf,
    pub decoder_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub idle_unload_secs: u64,
    pub num_threads: usize,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            encoder_path: PathBuf::from("models/whisper-tiny.en/encoder_model_quantized.onnx"),
            decoder_path: PathBuf::from(
                "models/whisper-tiny.en/decoder_model_merged_quantized.onnx",
            ),
            tokenizer_path: PathBuf::from("models/whisper-tiny.en/tokenizer.json"),
            idle_unload_secs: DEFAULT_IDLE_UNLOAD_SECS,
            num_threads: 2,
        }
    }
}

struct LoadedWhisperSessions {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
}

pub struct WhisperTranscriber {
    config: WhisperConfig,
    mel_filterbank: MelFilterbank,
    sessions: Arc<Mutex<Option<LoadedWhisperSessions>>>,
    last_used: Arc<Mutex<Instant>>,
}

impl WhisperTranscriber {
    pub fn new(config: WhisperConfig) -> Self {
        Self {
            config,
            mel_filterbank: MelFilterbank::new(),
            sessions: Arc::new(Mutex::new(None)),
            last_used: Arc::new(Mutex::new(Instant::now())),
        }
    }

    fn ensure_loaded(&self) -> SttResult<()> {
        let mut guard = self.sessions.lock();
        if guard.is_some() {
            *self.last_used.lock() = Instant::now();
            return Ok(());
        }

        if !self.config.encoder_path.exists() {
            return Err(SttError::ModelNotFound(format!(
                "Whisper encoder model not found at {}",
                self.config.encoder_path.display()
            )));
        }
        if !self.config.decoder_path.exists() {
            return Err(SttError::ModelNotFound(format!(
                "Whisper decoder model not found at {}",
                self.config.decoder_path.display()
            )));
        }
        if !self.config.tokenizer_path.exists() {
            return Err(SttError::ModelNotFound(format!(
                "Whisper tokenizer not found at {}",
                self.config.tokenizer_path.display()
            )));
        }

        info!(
            encoder = %self.config.encoder_path.display(),
            decoder = %self.config.decoder_path.display(),
            threads = self.config.num_threads,
            "loading Whisper ONNX transcription sessions into memory"
        );

        let encoder_bytes = std::fs::read(&self.config.encoder_path)?;
        let encoder = Session::builder()
            .map_err(|e| SttError::InferenceError(format!("encoder session builder: {e}")))?
            .with_intra_threads(self.config.num_threads)
            .map_err(|e| SttError::InferenceError(format!("setting encoder threads: {e}")))?
            .commit_from_memory(&encoder_bytes)
            .map_err(|e| SttError::InferenceError(format!("loading encoder ONNX: {e}")))?;

        let decoder_bytes = std::fs::read(&self.config.decoder_path)?;
        let decoder = Session::builder()
            .map_err(|e| SttError::InferenceError(format!("decoder session builder: {e}")))?
            .with_intra_threads(self.config.num_threads)
            .map_err(|e| SttError::InferenceError(format!("setting decoder threads: {e}")))?
            .commit_from_memory(&decoder_bytes)
            .map_err(|e| SttError::InferenceError(format!("loading decoder ONNX: {e}")))?;

        let tokenizer = Tokenizer::from_file(&self.config.tokenizer_path)
            .map_err(|e| SttError::TokenizerError(format!("failed to load tokenizer: {e}")))?;

        *guard = Some(LoadedWhisperSessions {
            encoder,
            decoder,
            tokenizer,
        });
        *self.last_used.lock() = Instant::now();
        Ok(())
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, audio: &[f32]) -> SttResult<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        self.ensure_loaded()?;
        let mut guard = self.sessions.lock();
        let sessions = guard.as_mut().ok_or_else(|| {
            SttError::TranscriptionFailed("Whisper sessions not loaded".to_string())
        })?;
        *self.last_used.lock() = Instant::now();

        // 1. Compute 80-channel log-mel spectrogram [1, 80, 3000]
        let mel_data = self.mel_filterbank.compute_whisper_mel(audio);
        let mel_tensor =
            Tensor::from_array(([1, N_MELS, WHISPER_FRAMES], mel_data.into_boxed_slice()))
                .map_err(|e| SttError::InferenceError(format!("mel tensor error: {e}")))?;

        // 2. Run Whisper Encoder
        let encoder_inputs = ort::inputs!["input_features" => mel_tensor];

        let mut encoder_outputs = sessions
            .encoder
            .run(encoder_inputs)
            .map_err(|e| SttError::InferenceError(format!("encoder run failed: {e}")))?;

        let hidden_states_val = encoder_outputs.remove("last_hidden_state").ok_or_else(|| {
            SttError::InferenceError("missing last_hidden_state from encoder".to_string())
        })?;

        let (hidden_shape, hidden_data) = hidden_states_val
            .try_extract_tensor::<f32>()
            .map_err(|e| SttError::InferenceError(format!("extracting hidden states: {e}")))?;

        // 3. Greedy Autoregressive Decoder Loop
        let mut generated_tokens: Vec<i64> =
            vec![SOT_TOKEN, EN_TOKEN, TRANSCRIBE_TOKEN, NO_TIMESTAMPS_TOKEN];
        let max_tokens = 64;

        for _ in 0..max_tokens {
            let seq_len = generated_tokens.len();
            let tokens_tensor =
                Tensor::from_array(([1, seq_len], generated_tokens.clone().into_boxed_slice()))
                    .map_err(|e| SttError::InferenceError(format!("decoder tokens tensor: {e}")))?;

            let hidden_tensor = Tensor::from_array((
                [hidden_shape[0], hidden_shape[1], hidden_shape[2]],
                hidden_data.to_vec().into_boxed_slice(),
            ))
            .map_err(|e| SttError::InferenceError(format!("rebuilding hidden tensor: {e}")))?;

            let decoder_inputs = ort::inputs![
                "input_ids" => tokens_tensor,
                "encoder_hidden_states" => hidden_tensor,
            ];

            let decoder_outputs = sessions
                .decoder
                .run(decoder_inputs)
                .map_err(|e| SttError::InferenceError(format!("decoder run failed: {e}")))?;

            let logits_tensor = decoder_outputs.get("logits").ok_or_else(|| {
                SttError::InferenceError("missing logits from decoder".to_string())
            })?;

            let (shape, logits_data) = logits_tensor
                .try_extract_tensor::<f32>()
                .map_err(|e| SttError::InferenceError(format!("extracting logits: {e}")))?;

            if shape.len() < 3 {
                break;
            }
            let vocab_size = shape[2] as usize;
            let last_token_logits = &logits_data[(seq_len - 1) * vocab_size..seq_len * vocab_size];

            // Argmax selection
            let mut best_id = 0i64;
            let mut best_val = f32::NEG_INFINITY;
            for (id, &val) in last_token_logits.iter().enumerate() {
                if val > best_val {
                    best_val = val;
                    best_id = id as i64;
                }
            }

            if best_id == EOT_TOKEN {
                break;
            }

            generated_tokens.push(best_id);
        }

        // 4. Decode generated token IDs to natural text (excluding prompt tokens)
        let content_tokens: Vec<u32> = generated_tokens
            .iter()
            .skip(4) // Skip initial prompt tokens
            .filter(|&&id| id != EOT_TOKEN)
            .map(|&id| id as u32)
            .collect();

        if content_tokens.is_empty() {
            return Ok(String::new());
        }

        let text = sessions
            .tokenizer
            .decode(&content_tokens, true)
            .map_err(|e| SttError::TokenizerError(format!("decode tokens: {e}")))?;

        Ok(text.trim().to_string())
    }

    fn is_loaded(&self) -> bool {
        self.sessions.lock().is_some()
    }

    fn unload(&self) {
        let mut guard = self.sessions.lock();
        if guard.is_some() {
            info!("unloading Whisper ONNX model from memory to reclaim RAM");
            *guard = None;
        }
    }

    fn maybe_unload_idle(&self) -> bool {
        let elapsed = self.last_used.lock().elapsed().as_secs();
        if elapsed >= self.config.idle_unload_secs && self.is_loaded() {
            self.unload();
            true
        } else {
            false
        }
    }
}
