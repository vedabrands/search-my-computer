use std::f32::consts::PI;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const MAX_RECORDING_SECONDS: usize = 45;
pub const MAX_SAMPLES: usize = TARGET_SAMPLE_RATE as usize * MAX_RECORDING_SECONDS;
pub const DEFAULT_SILENCE_TIMEOUT_SECS: f32 = 15.0;
pub const SILENCE_RMS_THRESHOLD: f32 = 0.008; // Roughly -42 dBFS

/// Converts interleaved multi-channel audio to mono by averaging channels.
pub fn stereo_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    let frames = interleaved.len() / ch;
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..ch {
            sum += interleaved[i * ch + c];
        }
        mono.push(sum / channels as f32);
    }
    mono
}

/// Resamples mono audio from `from_rate` to `to_rate` using linear/cubic interpolation.
pub fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let target_len = ((input.len() as f64) / ratio).round() as usize;
    let mut output = Vec::with_capacity(target_len);

    for i in 0..target_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;

        if idx + 1 < input.len() {
            // Linear interpolation
            let sample = input[idx] * (1.0 - frac) + input[idx + 1] * frac;
            output.push(sample);
        } else if idx < input.len() {
            output.push(input[idx]);
        }
    }
    output
}

/// Calculates the Root Mean Square (RMS) amplitude of audio samples.
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Converts linear RMS [0.0, 1.0] to normalized UI level [0.0, 1.0] with dynamic range compression.
pub fn rms_to_ui_level(rms: f32) -> f32 {
    if rms <= 0.0001 {
        return 0.0;
    }
    // Convert to dBFS (from -60dB to 0dB)
    let db = 20.0 * (rms + 1e-6).log10();
    let normalized = ((db + 50.0) / 50.0).clamp(0.0, 1.0);
    // Smooth quadratic curve for responsive meter aesthetics
    normalized.powf(1.4)
}

/// Detects if an audio buffer represents silence below the noise floor.
pub fn is_silence(samples: &[f32], threshold: f32) -> bool {
    let rms = calculate_rms(samples);
    rms < threshold
}

/// Generates a synthetic sine wave tone (useful for tests and audio cues).
pub fn generate_sine_wave(
    freq_hz: f32,
    duration_secs: f32,
    sample_rate: u32,
    volume: f32,
) -> Vec<f32> {
    let total_samples = (duration_secs * sample_rate as f32) as usize;
    let mut buffer = Vec::with_capacity(total_samples);
    for i in 0..total_samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * PI * freq_hz * t).sin() * volume;
        buffer.push(sample);
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stereo_to_mono() {
        let stereo = vec![0.5, 0.5, 1.0, 0.0, -0.5, 0.5];
        let mono = stereo_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < 1e-6);
        assert!((mono[1] - 0.5).abs() < 1e-6);
        assert!((mono[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_resampling() {
        let input = generate_sine_wave(440.0, 1.0, 48000, 0.8);
        let resampled = resample(&input, 48000, 16000);
        assert_eq!(resampled.len(), 16000);
        let rms_orig = calculate_rms(&input);
        let rms_resampled = calculate_rms(&resampled);
        assert!((rms_orig - rms_resampled).abs() < 0.05);
    }

    #[test]
    fn test_silence_detection() {
        let silence = vec![0.001; 1600];
        assert!(is_silence(&silence, SILENCE_RMS_THRESHOLD));

        let loud = generate_sine_wave(440.0, 0.1, 16000, 0.5);
        assert!(!is_silence(&loud, SILENCE_RMS_THRESHOLD));
    }
}
