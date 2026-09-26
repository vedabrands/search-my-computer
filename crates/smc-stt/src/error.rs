use thiserror::Error;

#[derive(Error, Debug)]
pub enum SttError {
    #[error("Audio device unavailable: {0}")]
    DeviceUnavailable(String),

    #[error("Microphone unavailable: {0}")]
    MicrophoneUnavailable(String),

    #[error("Microphone permission denied: {0}")]
    PermissionDenied(String),

    #[error("Audio stream error: {0}")]
    AudioStreamError(String),

    #[error("STT model file not found: {0}")]
    ModelNotFound(String),

    #[error("ONNX inference failed: {0}")]
    InferenceError(String),

    #[error("Audio transcription failed: {0}")]
    TranscriptionFailed(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Tokenizer error: {0}")]
    TokenizerError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type SttResult<T> = Result<T, SttError>;
