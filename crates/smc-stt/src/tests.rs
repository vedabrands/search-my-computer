use crate::audio::{
    DEFAULT_SILENCE_TIMEOUT_SECS, SILENCE_RMS_THRESHOLD, TARGET_SAMPLE_RATE, calculate_rms,
    generate_sine_wave, is_silence, resample, rms_to_ui_level,
};
use crate::capture::{AudioCaptureManager, AudioCaptureState, CaptureConfig};
use crate::error::SttResult;
use crate::mel::{MelFilterbank, N_MELS, WHISPER_FRAMES};
use crate::transcriber::{Transcriber, WhisperConfig, WhisperTranscriber};
use crate::wakeword::{WakeWordConfig, WakeWordEngine};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct MockTranscriber {
    return_text: String,
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, _audio: &[f32]) -> SttResult<String> {
        Ok(self.return_text.clone())
    }

    fn is_loaded(&self) -> bool {
        false
    }

    fn unload(&self) {}

    fn maybe_unload_idle(&self) -> bool {
        false
    }
}

#[test]
fn test_audio_resampling_and_rms() {
    let original_rate = 48000;
    let duration_secs = 1.0;
    let samples = generate_sine_wave(440.0, duration_secs, original_rate, 0.8);
    assert_eq!(samples.len(), 48000);

    let resampled = resample(&samples, original_rate, TARGET_SAMPLE_RATE);
    assert_eq!(resampled.len(), 16000);

    let rms = calculate_rms(&resampled);
    assert!(rms > 0.5 && rms < 0.6); // Expected ~0.565 for 0.8 sine wave amplitude

    let ui_level = rms_to_ui_level(rms);
    assert!(ui_level > 0.6 && ui_level <= 1.0);
}

#[test]
fn test_silence_detection() {
    let silence = vec![0.0001f32; 1600];
    assert!(is_silence(&silence, SILENCE_RMS_THRESHOLD));
    let rms = calculate_rms(&silence);
    assert_eq!(rms_to_ui_level(rms), 0.0);

    let speech = generate_sine_wave(300.0, 0.1, TARGET_SAMPLE_RATE, 0.5);
    assert!(!is_silence(&speech, SILENCE_RMS_THRESHOLD));
    let speech_rms = calculate_rms(&speech);
    assert!(rms_to_ui_level(speech_rms) > 0.4);
}

#[test]
fn test_mel_filterbank_whisper_dimensions() {
    let fb = MelFilterbank::new();
    let audio = generate_sine_wave(440.0, 2.0, TARGET_SAMPLE_RATE, 0.5);
    let mel = fb.compute_whisper_mel(&audio);

    assert_eq!(mel.len(), N_MELS * WHISPER_FRAMES);
}

#[test]
fn test_manual_start_and_stop_listening_pipeline() {
    let mock_transcriber = Arc::new(MockTranscriber {
        return_text: "where is my tax return".to_string(),
    });
    let ww_engine = Arc::new(WakeWordEngine::new(WakeWordConfig::default()));
    let config = CaptureConfig {
        wake_word_enabled: false,
        silence_timeout_secs: DEFAULT_SILENCE_TIMEOUT_SECS,
        max_duration_secs: 45.0,
        wake_word_phrase: "Kira".to_string(),
    };

    let manager = AudioCaptureManager::new(config, ww_engine, mock_transcriber);
    assert_eq!(manager.get_state(), AudioCaptureState::Idle);

    // Start manual recording
    manager.start_manual_recording().unwrap();
    match manager.get_state() {
        AudioCaptureState::Listening { .. } => {}
        other => panic!("expected Listening state, got {other:?}"),
    }

    // Ingest simulated audio
    let audio = generate_sine_wave(440.0, 0.5, 16000, 0.4);
    let transcription = manager.ingest_audio_samples(&audio, 16000, 1);
    assert!(transcription.is_none()); // Still active

    // Manual stop
    let result = manager.stop_listening_now().unwrap();
    assert_eq!(result, "where is my tax return");
    assert_eq!(
        manager.get_state(),
        AudioCaptureState::Done {
            transcription: "where is my tax return".to_string()
        }
    );
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
    let config = WhisperConfig {
        idle_unload_secs: 1, // 1 second for test
        ..Default::default()
    };
    let transcriber = WhisperTranscriber::new(config);

    assert!(!transcriber.is_loaded());

    // Trigger unload check when nothing loaded
    assert!(!transcriber.maybe_unload_idle());
    assert!(!transcriber.is_loaded());

    // Test callback invocation
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);
    let manager = AudioCaptureManager::new(
        CaptureConfig::default(),
        Arc::new(WakeWordEngine::new(WakeWordConfig::default())),
        Arc::new(MockTranscriber {
            return_text: "callback test".to_string(),
        }),
    );

    manager.set_on_transcription(move |text| {
        if text == "callback test" {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        }
    });

    manager.start_manual_recording().unwrap();
    let text = manager.stop_listening_now().unwrap();
    assert_eq!(text, "callback test");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}
