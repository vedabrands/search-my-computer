use crate::audio::{
    DEFAULT_SILENCE_TIMEOUT_SECS, MAX_RECORDING_SECONDS, MAX_SAMPLES, SILENCE_RMS_THRESHOLD,
    TARGET_SAMPLE_RATE, calculate_rms, resample, rms_to_ui_level, stereo_to_mono,
};
use crate::error::{SttError, SttResult};
use crate::transcriber::Transcriber;
use crate::wakeword::WakeWordEngine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", content = "data")]
pub enum AudioCaptureState {
    Idle,
    WakeWordArmed,
    Listening {
        duration_secs: f32,
        silence_secs: f32,
        max_secs: f32,
        level: f32, // Normalized UI audio level [0.0, 1.0]
    },
    Transcribing,
    Done {
        transcription: String,
    },
    Error {
        message: String,
    },
}

pub struct CaptureConfig {
    pub wake_word_enabled: bool,
    pub silence_timeout_secs: f32,
    pub max_duration_secs: f32,
    pub wake_word_phrase: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            wake_word_enabled: false, // Off by default for privacy
            silence_timeout_secs: DEFAULT_SILENCE_TIMEOUT_SECS,
            max_duration_secs: MAX_RECORDING_SECONDS as f32,
            wake_word_phrase: "Kira".to_string(),
        }
    }
}

pub struct AudioStream(Option<cpal::Stream>);
unsafe impl Send for AudioStream {}
unsafe impl Sync for AudioStream {}

impl AudioStream {
    pub fn new(stream: cpal::Stream) -> Self {
        Self(Some(stream))
    }
    pub fn stop(&mut self) {
        self.0 = None;
    }
}

type WakeWordCallback = Arc<dyn Fn() + Send + Sync>;
type TranscriptionCallback = Arc<dyn Fn(String) + Send + Sync>;

pub struct AudioCaptureManager {
    config: Arc<Mutex<CaptureConfig>>,
    state: Arc<Mutex<AudioCaptureState>>,
    wake_word_engine: Arc<WakeWordEngine>,
    transcriber: Arc<dyn Transcriber>,
    is_capturing: Arc<AtomicBool>,
    is_transcribing: Arc<AtomicBool>,
    wake_word_paused_until: Arc<Mutex<Option<Instant>>>,
    recording_buffer: Arc<Mutex<Vec<f32>>>,
    capture_start_time: Arc<Mutex<Option<Instant>>>,
    last_voice_activity: Arc<Mutex<Option<Instant>>>,
    current_level: Arc<Mutex<f32>>,
    on_wake_word: Arc<Mutex<Option<WakeWordCallback>>>,
    on_transcription: Arc<Mutex<Option<TranscriptionCallback>>>,
}

impl AudioCaptureManager {
    pub fn new(
        config: CaptureConfig,
        wake_word_engine: Arc<WakeWordEngine>,
        transcriber: Arc<dyn Transcriber>,
    ) -> Self {
        let initial_state = if config.wake_word_enabled {
            AudioCaptureState::WakeWordArmed
        } else {
            AudioCaptureState::Idle
        };

        Self {
            config: Arc::new(Mutex::new(config)),
            state: Arc::new(Mutex::new(initial_state)),
            wake_word_engine,
            transcriber,
            is_capturing: Arc::new(AtomicBool::new(false)),
            is_transcribing: Arc::new(AtomicBool::new(false)),
            wake_word_paused_until: Arc::new(Mutex::new(None)),
            recording_buffer: Arc::new(Mutex::new(Vec::with_capacity(MAX_SAMPLES))),
            capture_start_time: Arc::new(Mutex::new(None)),
            last_voice_activity: Arc::new(Mutex::new(None)),
            current_level: Arc::new(Mutex::new(0.0)),
            on_wake_word: Arc::new(Mutex::new(None)),
            on_transcription: Arc::new(Mutex::new(None)),
        }
    }

    /// Registers a callback to be invoked whenever wake word is triggered.
    pub fn set_on_wake_word<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *self.on_wake_word.lock() = Some(Arc::new(callback));
    }

    /// Registers a callback to be invoked whenever a transcription completes.
    pub fn set_on_transcription<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        *self.on_transcription.lock() = Some(Arc::new(callback));
    }

    /// Gets current capture state for UI polling and synchronization.
    pub fn get_state(&self) -> AudioCaptureState {
        self.state.lock().clone()
    }

    /// Checks if wake word is temporarily paused.
    pub fn is_wake_word_paused(&self) -> bool {
        let guard = self.wake_word_paused_until.lock();
        if let Some(until) = *guard
            && Instant::now() < until
        {
            return true;
        }
        false
    }

    /// Pauses wake word detection for a specified duration (e.g. 15m, 1h).
    pub fn pause_wake_word(&self, duration: Duration) {
        let resume_at = Instant::now() + duration;
        *self.wake_word_paused_until.lock() = Some(resume_at);
        info!(
            duration_secs = duration.as_secs(),
            "wake word listener paused"
        );
        if *self.state.lock() == AudioCaptureState::WakeWordArmed {
            *self.state.lock() = AudioCaptureState::Idle;
        }
    }

    /// Resumes wake word detection immediately.
    pub fn resume_wake_word(&self) {
        *self.wake_word_paused_until.lock() = None;
        if self.config.lock().wake_word_enabled && !self.is_capturing.load(Ordering::SeqCst) {
            *self.state.lock() = AudioCaptureState::WakeWordArmed;
        }
    }

    /// Enables or disables wake word listening in settings.
    pub fn set_wake_word_enabled(&self, enabled: bool) {
        let mut cfg = self.config.lock();
        cfg.wake_word_enabled = enabled;
        if enabled {
            *self.wake_word_paused_until.lock() = None;
            if !self.is_capturing.load(Ordering::SeqCst) {
                *self.state.lock() = AudioCaptureState::WakeWordArmed;
            }
        } else {
            if *self.state.lock() == AudioCaptureState::WakeWordArmed {
                *self.state.lock() = AudioCaptureState::Idle;
            }
        }
    }

    /// Starts manual recording (via mic button or hotkey).
    pub fn start_manual_recording(&self) -> SttResult<()> {
        if self.is_capturing.load(Ordering::SeqCst) {
            return Ok(()); // Already capturing
        }
        self.begin_active_capture();
        Ok(())
    }

    /// Begins active speech capture buffer collection.
    fn begin_active_capture(&self) {
        self.recording_buffer.lock().clear();
        *self.capture_start_time.lock() = Some(Instant::now());
        *self.last_voice_activity.lock() = Some(Instant::now());
        self.is_capturing.store(true, Ordering::SeqCst);

        *self.state.lock() = AudioCaptureState::Listening {
            duration_secs: 0.0,
            silence_secs: 0.0,
            max_secs: self.config.lock().max_duration_secs,
            level: 0.0,
        };
        info!("voice capture started");
    }

    /// Manually stops recording immediately and initiates transcription.
    pub fn stop_listening_now(&self) -> SttResult<String> {
        if !self.is_capturing.load(Ordering::SeqCst) {
            if let AudioCaptureState::Done { ref transcription } = *self.state.lock() {
                return Ok(transcription.clone());
            }
            return Ok(String::new());
        }

        self.is_capturing.store(false, Ordering::SeqCst);
        *self.state.lock() = AudioCaptureState::Transcribing;
        self.is_transcribing.store(true, Ordering::SeqCst);

        // Take in-memory audio samples
        let samples: Vec<f32> = {
            let mut buf = self.recording_buffer.lock();
            let data = buf.clone();
            buf.clear(); // Immediately free memory buffer
            data
        };

        let transcriber = Arc::clone(&self.transcriber);
        let result = transcriber.transcribe(&samples);

        self.is_transcribing.store(false, Ordering::SeqCst);

        match result {
            Ok(text) => {
                *self.state.lock() = AudioCaptureState::Done {
                    transcription: text.clone(),
                };
                info!(
                    token_count = text.split_whitespace().count(),
                    "audio transcription completed"
                );
                if let Some(ref cb) = *self.on_transcription.lock() {
                    cb(text.clone());
                }
                Ok(text)
            }
            Err(e) => {
                let err_msg = e.to_string();
                error!(error = %err_msg, "transcription failed");
                *self.state.lock() = AudioCaptureState::Error {
                    message: err_msg.clone(),
                };
                Err(e)
            }
        }
    }

    /// Feeds incoming audio samples into the capture manager.
    /// Handles both wake word detection and active recording accumulation.
    pub fn ingest_audio_samples(
        &self,
        samples: &[f32],
        sample_rate: u32,
        channels: u16,
    ) -> Option<String> {
        if samples.is_empty() {
            return None;
        }

        // Convert to mono and 16 kHz
        let mono = stereo_to_mono(samples, channels);
        let resampled_16k = resample(&mono, sample_rate, TARGET_SAMPLE_RATE);

        let rms = calculate_rms(&resampled_16k);
        let ui_level = rms_to_ui_level(rms);
        *self.current_level.lock() = ui_level;

        if self.is_capturing.load(Ordering::SeqCst) {
            // --- Active Listening Mode ---
            let mut buf = self.recording_buffer.lock();
            if buf.len() + resampled_16k.len() <= MAX_SAMPLES {
                buf.extend_from_slice(&resampled_16k);
            }
            let total_samples = buf.len();
            drop(buf);

            let now = Instant::now();
            let start = *self.capture_start_time.lock();
            let duration_secs = start
                .map(|s| now.duration_since(s).as_secs_f32())
                .unwrap_or(0.0);

            // Update voice activity timestamp if loud enough
            if rms > SILENCE_RMS_THRESHOLD {
                *self.last_voice_activity.lock() = Some(now);
            }

            let silence_secs = self
                .last_voice_activity
                .lock()
                .map(|t| now.duration_since(t).as_secs_f32())
                .unwrap_or(0.0);

            let max_secs = self.config.lock().max_duration_secs;
            let silence_timeout = self.config.lock().silence_timeout_secs;

            *self.state.lock() = AudioCaptureState::Listening {
                duration_secs,
                silence_secs,
                max_secs,
                level: ui_level,
            };

            // Auto-stop conditions:
            // 1. 15 seconds of continuous silence (after initial speech or immediately)
            // 2. 45 seconds hard cap reached
            if silence_secs >= silence_timeout
                || duration_secs >= max_secs
                || total_samples >= MAX_SAMPLES
            {
                info!(
                    duration = duration_secs,
                    silence = silence_secs,
                    "auto-stopping audio capture"
                );
                if let Ok(transcription) = self.stop_listening_now() {
                    return Some(transcription);
                }
            }
        } else if self.config.lock().wake_word_enabled && !self.is_wake_word_paused() {
            // --- Background Wake-Word Detection Mode ---
            if *self.state.lock() != AudioCaptureState::WakeWordArmed {
                *self.state.lock() = AudioCaptureState::WakeWordArmed;
            }

            if let Ok(Some(confidence)) = self.wake_word_engine.process_audio(&resampled_16k) {
                info!(
                    confidence = confidence,
                    "wake word detected - activating capture"
                );
                self.begin_active_capture();
                if let Some(ref cb) = *self.on_wake_word.lock() {
                    cb();
                }
            }
        }

        None
    }

    /// Resets state back to Idle or WakeWordArmed.
    pub fn reset_state(&self) {
        self.is_capturing.store(false, Ordering::SeqCst);
        self.is_transcribing.store(false, Ordering::SeqCst);
        self.recording_buffer.lock().clear();

        let cfg = self.config.lock();
        if cfg.wake_word_enabled && !self.is_wake_word_paused() {
            *self.state.lock() = AudioCaptureState::WakeWordArmed;
        } else {
            *self.state.lock() = AudioCaptureState::Idle;
        }
    }

    /// Spawns a CPAL microphone input stream feeding into this capture manager.
    /// Returns the active `AudioStream` or an `SttError` if no microphone is found or permission is denied.
    pub fn spawn_mic_stream(manager: Arc<Self>) -> SttResult<AudioStream> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| {
            SttError::MicrophoneUnavailable("no default audio input device found".to_string())
        })?;

        let config = device.default_input_config().map_err(|e| {
            SttError::MicrophoneUnavailable(format!("failed to get default input config: {e}"))
        })?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        info!(
            sample_rate = sample_rate,
            channels = channels,
            "initializing CPAL microphone input stream"
        );

        let err_fn = move |err| {
            error!(error = %err, "CPAL audio input stream error");
        };

        let mgr = Arc::clone(&manager);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        mgr.ingest_audio_samples(data, sample_rate, channels);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| {
                    SttError::MicrophoneUnavailable(format!("building f32 input stream: {e}"))
                })?,
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| {
                        let f32_samples: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        mgr.ingest_audio_samples(&f32_samples, sample_rate, channels);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| {
                    SttError::MicrophoneUnavailable(format!("building i16 input stream: {e}"))
                })?,
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &_| {
                        let f32_samples: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0))
                            .collect();
                        mgr.ingest_audio_samples(&f32_samples, sample_rate, channels);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| {
                    SttError::MicrophoneUnavailable(format!("building u16 input stream: {e}"))
                })?,
            format => {
                return Err(SttError::MicrophoneUnavailable(format!(
                    "unsupported audio sample format: {format:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| SttError::MicrophoneUnavailable(format!("starting audio stream: {e}")))?;

        Ok(AudioStream::new(stream))
    }
}
