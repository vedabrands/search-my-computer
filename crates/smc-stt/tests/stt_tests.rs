use smc_stt::audio::{
    SILENCE_RMS_THRESHOLD, TARGET_SAMPLE_RATE, calculate_rms, generate_sine_wave, is_silence,
    resample, rms_to_ui_level,
};
use smc_stt::capture::{AudioCaptureManager, AudioCaptureState, CaptureConfig};
use smc_stt::error::SttResult;
use smc_stt::mel::MelFilterbank;
use smc_stt::transcriber::{Transcriber, WhisperConfig, WhisperTranscriber};
use smc_stt::wakeword::{WakeWordConfig, WakeWordEngine};
use std::sync::Arc;
use std::time::Duration;

/// Mock transcriber for deterministic testing of capture pipeline.
struct MockTranscriber {
    return_text: String,
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, audio: &[f32]) -> SttResult<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }
        Ok(self.return_text.clone())
    }

    fn is_loaded(&self) -> bool {
        true
    }

    fn unload(&self) {}

    fn maybe_unload_idle(&self) -> bool {
        false
    }
}

#[test]
fn test_audio_resampling_and_rms() {
    let raw_48k = generate_sine_wave(440.0, 0.5, 48000, 0.5);
    let resampled = resample(&raw_48k, 48000, TARGET_SAMPLE_RATE);
    assert_eq!(resampled.len(), 8000);

    let rms = calculate_rms(&resampled);
    assert!(rms > 0.3 && rms < 0.4);

    let level = rms_to_ui_level(rms);
    assert!(level > 0.0 && level <= 1.0);
}

#[test]
fn test_silence_detection() {
    let silence = vec![0.0f32; 16000];
    assert!(is_silence(&silence, SILENCE_RMS_THRESHOLD));
    let rms = calculate_rms(&silence);
    assert_eq!(rms, 0.0);
    assert_eq!(rms_to_ui_level(rms), 0.0);
}

#[test]
fn test_mel_filterbank_whisper_dimensions() {
    let fb = MelFilterbank::new();
    let audio = generate_sine_wave(1000.0, 2.0, TARGET_SAMPLE_RATE, 0.7);
    let mel = fb.compute_whisper_mel(&audio);
    assert_eq!(mel.len(), 80 * 3000);
}

#[test]
fn test_manual_start_and_stop_listening_pipeline() {
    let mock_transcriber = Arc::new(MockTranscriber {
        return_text: "find my tax return pdf".to_string(),
    });
    let ww_engine = Arc::new(WakeWordEngine::new(WakeWordConfig::default()));
    let manager = AudioCaptureManager::new(CaptureConfig::default(), ww_engine, mock_transcriber);

    assert_eq!(manager.get_state(), AudioCaptureState::Idle);

    // 1. Start manual recording
    manager.start_manual_recording().unwrap();
    match manager.get_state() {
        AudioCaptureState::Listening { .. } => {}
        other => panic!("expected Listening state, got {other:?}"),
    }

    // 2. Feed 1 second of audio
    let audio = generate_sine_wave(300.0, 1.0, 16000, 0.4);
    manager.ingest_audio_samples(&audio, 16000, 1);

    // 3. Stop listening manually immediately
    let transcription = manager.stop_listening_now().unwrap();
    assert_eq!(transcription, "find my tax return pdf");

    match manager.get_state() {
        AudioCaptureState::Done { ref transcription } => {
            assert_eq!(transcription, "find my tax return pdf");
        }
        other => panic!("expected Done state, got {other:?}"),
    }
}

#[test]
fn test_auto_stop_on_silence_timeout() {
    let mock_transcriber = Arc::new(MockTranscriber {
        return_text: "auto stopped text".to_string(),
    });
    let ww_engine = Arc::new(WakeWordEngine::new(WakeWordConfig::default()));
    let config = CaptureConfig {
        silence_timeout_secs: 0.05, // 50ms for instant test
        ..Default::default()
    };

    let manager = AudioCaptureManager::new(config, ww_engine, mock_transcriber);
    manager.start_manual_recording().unwrap();

    // Ingest silence
    let silence = vec![0.0001f32; 1600];
    std::thread::sleep(Duration::from_millis(60));
    let result = manager.ingest_audio_samples(&silence, 16000, 1);
    assert_eq!(result, Some("auto stopped text".to_string()));
}

#[test]
fn test_pause_and_resume_wake_word() {
    let mock_transcriber = Arc::new(MockTranscriber {
        return_text: "hello".to_string(),
    });
    let ww_engine = Arc::new(WakeWordEngine::new(WakeWordConfig::default()));
    let config = CaptureConfig {
        wake_word_enabled: true,
        ..Default::default()
    };

    let manager = AudioCaptureManager::new(config, ww_engine, mock_transcriber);
    assert_eq!(manager.get_state(), AudioCaptureState::WakeWordArmed);

    // Pause for 1 hour
    manager.pause_wake_word(Duration::from_secs(3600));
    assert!(manager.is_wake_word_paused());
    assert_eq!(manager.get_state(), AudioCaptureState::Idle);

    // Resume
    manager.resume_wake_word();
    assert!(!manager.is_wake_word_paused());
    assert_eq!(manager.get_state(), AudioCaptureState::WakeWordArmed);
}

#[test]
fn test_transcriber_lazy_load_and_unload() {
    let config = WhisperConfig::default();
    let transcriber = WhisperTranscriber::new(config);
    assert!(!transcriber.is_loaded());

    // Calling unload on uninitialized session is a safe no-op
    transcriber.unload();
    assert!(!transcriber.is_loaded());
}
