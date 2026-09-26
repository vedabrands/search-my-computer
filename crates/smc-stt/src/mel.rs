use std::f32::consts::PI;

pub const N_FFT: usize = 400;
pub const HOP_LENGTH: usize = 160;
pub const N_MELS: usize = 80;
pub const SAMPLE_RATE: usize = 16_000;
pub const WHISPER_FRAMES: usize = 3000; // 30 seconds at 100 fps (10ms hop)

/// Computes the Hann window for N_FFT.
fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos()))
        .collect()
}

/// Converts frequency in Hz to Mel scale (Slaney / HTK formulation).
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Converts Mel scale value back to Hz.
fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0f32.powf(mel / 2595.0) - 1.0)
}

/// Precomputed Mel Filterbank structure.
pub struct MelFilterbank {
    filters: Vec<Vec<f32>>, // [N_MELS, N_FFT / 2 + 1]
    window: Vec<f32>,
    cos_table: Vec<Vec<f32>>,
    sin_table: Vec<Vec<f32>>,
}

impl MelFilterbank {
    pub fn new() -> Self {
        let n_freqs = N_FFT / 2 + 1; // 201 bins
        let min_mel = hz_to_mel(0.0);
        let max_mel = hz_to_mel(SAMPLE_RATE as f32 / 2.0);

        // N_MELS + 2 linearly spaced points in Mel scale
        let mel_points: Vec<f32> = (0..=N_MELS + 1)
            .map(|i| min_mel + i as f32 * (max_mel - min_mel) / (N_MELS + 1) as f32)
            .collect();

        let hz_points: Vec<f32> = mel_points.into_iter().map(mel_to_hz).collect();
        let bin_points: Vec<f32> = hz_points
            .into_iter()
            .map(|hz| (N_FFT as f32 + 1.0) * hz / SAMPLE_RATE as f32)
            .collect();

        let mut filters = vec![vec![0.0f32; n_freqs]; N_MELS];

        for m in 0..N_MELS {
            let left = bin_points[m];
            let center = bin_points[m + 1];
            let right = bin_points[m + 2];

            for (k, val) in filters[m].iter_mut().enumerate().take(n_freqs) {
                let k_f = k as f32;
                if k_f >= left && k_f <= center && center > left {
                    *val = (k_f - left) / (center - left);
                } else if k_f >= center && k_f <= right && right > center {
                    *val = (right - k_f) / (right - center);
                }
            }
        }

        // Precompute twiddle tables for N_FFT DFT (k in 0..201, n in 0..400)
        let mut cos_table = vec![vec![0.0f32; N_FFT]; n_freqs];
        let mut sin_table = vec![vec![0.0f32; N_FFT]; n_freqs];
        for k in 0..n_freqs {
            for n in 0..N_FFT {
                let angle = 2.0 * PI * (k * n) as f32 / N_FFT as f32;
                cos_table[k][n] = angle.cos();
                sin_table[k][n] = -angle.sin();
            }
        }

        Self {
            filters,
            window: hann_window(N_FFT),
            cos_table,
            sin_table,
        }
    }

    /// Computes 80-channel log-mel spectrogram for audio samples, outputting shape [80, 3000].
    /// Padded or clamped to 3000 frames (30 seconds) for standard Whisper encoder.
    pub fn compute_whisper_mel(&self, audio: &[f32]) -> Vec<f32> {
        let n_freqs = N_FFT / 2 + 1;
        let pad_amount = N_FFT / 2;

        // Reflect padding
        let mut padded = Vec::with_capacity(audio.len() + 2 * pad_amount);
        for i in (1..=pad_amount).rev() {
            if i < audio.len() {
                padded.push(audio[i]);
            } else {
                padded.push(0.0);
            }
        }
        padded.extend_from_slice(audio);
        for i in 0..pad_amount {
            if audio.len() >= 2 + i {
                padded.push(audio[audio.len() - 2 - i]);
            } else {
                padded.push(0.0);
            }
        }

        let num_frames = if padded.len() >= N_FFT {
            (padded.len() - N_FFT) / HOP_LENGTH + 1
        } else {
            0
        };

        // Output array: shape [N_MELS, WHISPER_FRAMES] in row-major format (flattened [80, 3000])
        let mut mel_spec = vec![0.0f32; N_MELS * WHISPER_FRAMES];
        let frames_to_compute = num_frames.min(WHISPER_FRAMES);

        let mut stft_magnitudes = vec![0.0f32; n_freqs];

        for f in 0..frames_to_compute {
            let offset = f * HOP_LENGTH;
            let frame = &padded[offset..offset + N_FFT];

            // Compute DFT magnitude spectrum
            for (k, val) in stft_magnitudes.iter_mut().enumerate().take(n_freqs) {
                let mut real = 0.0f32;
                let mut imag = 0.0f32;
                let cos_k = &self.cos_table[k];
                let sin_k = &self.sin_table[k];

                for n in 0..N_FFT {
                    let sample = frame[n] * self.window[n];
                    real += sample * cos_k[n];
                    imag += sample * sin_k[n];
                }
                *val = real * real + imag * imag;
            }

            // Apply mel filters
            for m in 0..N_MELS {
                let filter = &self.filters[m];
                let mut mel_energy = 0.0f32;
                for k in 0..n_freqs {
                    mel_energy += filter[k] * stft_magnitudes[k];
                }
                // Log mel: log10(max(energy, 1e-5))
                let log_val = (mel_energy.max(1e-5)).log10();
                mel_spec[m * WHISPER_FRAMES + f] = log_val;
            }
        }

        // Fill remaining frames with min log value
        let min_val = -5.0f32;
        for m in 0..N_MELS {
            for f in frames_to_compute..WHISPER_FRAMES {
                mel_spec[m * WHISPER_FRAMES + f] = min_val;
            }
        }

        // Global normalization: max(x, max(x) - 8.0), (x + 4.0) / 4.0
        let mut max_val = f32::NEG_INFINITY;
        for &v in &mel_spec {
            if v > max_val {
                max_val = v;
            }
        }

        let floor_val = max_val - 8.0;
        for v in &mut mel_spec {
            let clamped = v.max(floor_val);
            *v = (clamped + 4.0) / 4.0;
        }

        mel_spec
    }
}

impl Default for MelFilterbank {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mel_filterbank_shape() {
        let fb = MelFilterbank::new();
        assert_eq!(fb.filters.len(), 80);
        assert_eq!(fb.filters[0].len(), 201);
    }

    #[test]
    fn test_compute_whisper_mel() {
        let fb = MelFilterbank::new();
        let dummy_audio = vec![0.0f32; 16000]; // 1 second of silence
        let mel = fb.compute_whisper_mel(&dummy_audio);
        assert_eq!(mel.len(), 80 * 3000);
    }
}
