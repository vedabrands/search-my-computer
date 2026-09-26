pub mod audio;
pub mod capture;
pub mod error;
pub mod mel;
pub mod transcriber;
pub mod wakeword;

pub use audio::{
    DEFAULT_SILENCE_TIMEOUT_SECS, MAX_RECORDING_SECONDS, MAX_SAMPLES, SILENCE_RMS_THRESHOLD,
    TARGET_SAMPLE_RATE, calculate_rms, generate_sine_wave, is_silence, resample, rms_to_ui_level,
    stereo_to_mono,
};
pub use capture::{AudioCaptureManager, AudioCaptureState, CaptureConfig};
pub use error::{SttError, SttResult};
pub use mel::{MelFilterbank, N_FFT, N_MELS, SAMPLE_RATE, WHISPER_FRAMES};
pub use transcriber::{
    DEFAULT_IDLE_UNLOAD_SECS, EN_TOKEN, EOT_TOKEN, NO_TIMESTAMPS_TOKEN, SOT_TOKEN,
    TRANSCRIBE_TOKEN, Transcriber, WhisperConfig, WhisperTranscriber,
};
pub use wakeword::{
    DEFAULT_WAKE_THRESHOLD, EMBEDDING_DIM, EMBEDDING_FRAMES, WAKE_WORD_FRAME_SAMPLES,
    WakeWordConfig, WakeWordEngine,
};

#[cfg(test)]
mod tests;
