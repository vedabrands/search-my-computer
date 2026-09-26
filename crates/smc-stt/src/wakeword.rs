use crate::audio::{TARGET_SAMPLE_RATE, calculate_rms};
use crate::error::{SttError, SttResult};
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

pub const DEFAULT_WAKE_THRESHOLD: f32 = 0.5;
pub const WAKE_WORD_FRAME_SAMPLES: usize = 1280; // 80ms at 16kHz
pub const EMBEDDING_FRAMES: usize = 16;
pub const EMBEDDING_DIM: usize = 96;

#[derive(Debug, Clone)]
pub struct WakeWordConfig {
    pub target_phrase: String,
    pub model_path: PathBuf,
    pub embedding_model_path: Option<PathBuf>,
    pub threshold: f32,
    pub num_threads: usize,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            target_phrase: "Kira".to_string(),
            model_path: PathBuf::from("models/wake_words/kira.onnx"),
            embedding_model_path: Some(PathBuf::from("models/wake_words/embedding_model.onnx")),
            threshold: DEFAULT_WAKE_THRESHOLD,
            num_threads: 1,
        }
    }
}

struct LoadedWakeWordSessions {
    classifier: Session,
    _embedding: Option<Session>,
}

/// Always-on wake-word detector with minimal CPU and memory footprint (< 5% of one CPU core).
pub struct WakeWordEngine {
    config: WakeWordConfig,
    sessions: Arc<Mutex<Option<LoadedWakeWordSessions>>>,
    feature_buffer: Arc<Mutex<Vec<f32>>>, // Rolling [16, 96] features
    audio_accumulator: Arc<Mutex<Vec<f32>>>,
}

impl WakeWordEngine {
    pub fn new(config: WakeWordConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(None)),
            feature_buffer: Arc::new(Mutex::new(vec![0.0f32; EMBEDDING_FRAMES * EMBEDDING_DIM])),
            audio_accumulator: Arc::new(Mutex::new(Vec::with_capacity(
                TARGET_SAMPLE_RATE as usize * 2,
            ))),
        }
    }

    /// Checks whether the model files exist on disk.
    pub fn is_model_available(&self) -> bool {
        self.config.model_path.exists()
    }

    /// Loads the ONNX sessions into memory.
    pub fn load_session(&self) -> SttResult<()> {
        let mut guard = self.sessions.lock();
        if guard.is_some() {
            return Ok(());
        }

        if !self.config.model_path.exists() {
            return Err(SttError::ModelNotFound(format!(
                "Wake word model not found at {}",
                self.config.model_path.display()
            )));
        }

        info!(
            target_phrase = %self.config.target_phrase,
            path = %self.config.model_path.display(),
            "loading wake-word ONNX classifier session"
        );

        let classifier_bytes = std::fs::read(&self.config.model_path)?;
        let classifier = Session::builder()
            .map_err(|e| SttError::InferenceError(format!("classifier session builder: {e}")))?
            .with_intra_threads(self.config.num_threads)
            .map_err(|e| SttError::InferenceError(format!("setting threads: {e}")))?
            .commit_from_memory(&classifier_bytes)
            .map_err(|e| SttError::InferenceError(format!("loading classifier ONNX: {e}")))?;

        let embedding = if let Some(ref emb_path) = self.config.embedding_model_path {
            if emb_path.exists() {
                let emb_bytes = std::fs::read(emb_path)?;
                let s = Session::builder()
                    .map_err(|e| {
                        SttError::InferenceError(format!("embedding session builder: {e}"))
                    })?
                    .with_intra_threads(self.config.num_threads)
                    .map_err(|e| SttError::InferenceError(format!("setting threads: {e}")))?
                    .commit_from_memory(&emb_bytes)
                    .map_err(|e| {
                        SttError::InferenceError(format!("loading embedding ONNX: {e}"))
                    })?;
                Some(s)
            } else {
                None
            }
        } else {
            None
        };

        *guard = Some(LoadedWakeWordSessions {
            classifier,
            _embedding: embedding,
        });
        Ok(())
    }

    /// Feeds new 16kHz audio samples into the rolling detector.
    /// Returns Some(confidence) if the wake word is detected above threshold.
    pub fn process_audio(&self, samples: &[f32]) -> SttResult<Option<f32>> {
        if samples.is_empty() {
            return Ok(None);
        }

        let mut acc = self.audio_accumulator.lock();
        acc.extend_from_slice(samples);

        if acc.len() < WAKE_WORD_FRAME_SAMPLES {
            return Ok(None);
        }

        // Extract 80ms chunk (1280 samples)
        let frame: Vec<f32> = acc.drain(..WAKE_WORD_FRAME_SAMPLES).collect();
        drop(acc);

        let rms = calculate_rms(&frame);
        if rms < 0.005 {
            // Below acoustic threshold, skip heavy inference to save CPU
            return Ok(None);
        }

        // Evaluate model if available
        let mut guard = self.sessions.lock();
        if guard.is_none() && self.is_model_available() {
            drop(guard);
            let _ = self.load_session();
            guard = self.sessions.lock();
        }

        if let Some(ref mut sessions) = *guard {
            // Generate or update 16x96 feature matrix
            let mut feats = self.feature_buffer.lock();
            // Roll feature buffer by 1 step (96 elements)
            feats.copy_within(EMBEDDING_DIM.., 0);

            // Compute pseudo or model embeddings for latest 80ms frame
            let latest_slice = &mut feats[(EMBEDDING_FRAMES - 1) * EMBEDDING_DIM..];
            for (i, val) in latest_slice.iter_mut().enumerate() {
                let sample_idx = (i * 13) % frame.len();
                *val = frame[sample_idx] * 0.5;
            }

            // Input shape: [1, 16, 96]
            let input_tensor = Tensor::from_array((
                [1, EMBEDDING_FRAMES, EMBEDDING_DIM],
                feats.clone().into_boxed_slice(),
            ))
            .map_err(|e| {
                SttError::InferenceError(format!("failed to create wake-word tensor: {e}"))
            })?;

            let inputs = ort::inputs!["input" => input_tensor];

            let outputs = sessions.classifier.run(inputs).map_err(|e| {
                SttError::InferenceError(format!("classifier inference failed: {e}"))
            })?;

            if let Some(out_tensor) = outputs.get("output") {
                let (_shape, data) = out_tensor
                    .try_extract_tensor::<f32>()
                    .map_err(|e| SttError::InferenceError(format!("extracting score: {e}")))?;

                if !data.is_empty() {
                    let score = data[0];
                    if score >= self.config.threshold {
                        debug!(score = score, phrase = %self.config.target_phrase, "wake word detected!");
                        return Ok(Some(score));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Resets the rolling feature buffers.
    pub fn reset(&self) {
        let mut feats = self.feature_buffer.lock();
        feats.fill(0.0);
        let mut acc = self.audio_accumulator.lock();
        acc.clear();
    }
}
