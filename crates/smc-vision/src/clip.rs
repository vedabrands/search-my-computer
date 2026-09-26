use half::f16;
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, warn};

/// Embedding dimensions for CLIP ViT-B/32 models (512 dimensions).
pub const CLIP_EMBEDDING_DIMS: usize = 512;

/// Configuration for the optional local CPU ONNX CLIP engine.
#[derive(Debug, Clone)]
pub struct ClipConfig {
    pub visual_model_path: PathBuf,
    pub text_model_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub num_threads: usize,
}

impl Default for ClipConfig {
    fn default() -> Self {
        let models_dir = PathBuf::from("models").join("vision");
        Self {
            visual_model_path: models_dir.join("clip_visual.onnx"),
            text_model_path: models_dir.join("clip_text.onnx"),
            tokenizer_path: models_dir.join("tokenizer.json"),
            num_threads: 2,
        }
    }
}

impl ClipConfig {
    pub fn new(models_dir: &Path) -> Self {
        let dir = models_dir.join("vision");
        Self {
            visual_model_path: dir.join("clip_visual.onnx"),
            text_model_path: dir.join("clip_text.onnx"),
            tokenizer_path: dir.join("tokenizer.json"),
            num_threads: 2,
        }
    }
}

/// Helper to safely load an ONNX session from disk with thread configuration.
fn load_session(path: &Path, num_threads: usize) -> Option<Session> {
    if !path.exists() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let builder = Session::builder().ok()?;
    let mut builder = builder.with_intra_threads(num_threads).ok()?;
    builder.commit_from_memory(&bytes).ok()
}

/// Extract L2-normalized f16 embedding from ONNX session outputs.
fn extract_l2_normalized_embedding(outputs: ort::session::SessionOutputs) -> Option<Vec<f16>> {
    let (_name, val) = outputs.into_iter().next()?;
    let (_out_shape, out_data) = val.try_extract_tensor::<f32>().ok()?;

    let mut embedding = Vec::with_capacity(CLIP_EMBEDDING_DIMS);
    let mut sum_squares = 0.0f32;

    for &val in out_data.iter().take(CLIP_EMBEDDING_DIMS) {
        embedding.push(f16::from_f32(val));
        sum_squares += val * val;
    }

    while embedding.len() < CLIP_EMBEDDING_DIMS {
        embedding.push(f16::from_f32(0.0));
    }

    let norm = sum_squares.sqrt();
    if norm > 0.0 {
        for val in &mut embedding {
            *val = f16::from_f32(val.to_f32() / norm);
        }
    }

    Some(embedding)
}

/// Loaded ONNX sessions for CLIP visual and text encoders.
struct LoadedClipSessions {
    visual_session: Option<Session>,
    text_session: Option<Session>,
    #[allow(dead_code)]
    vocab: Option<serde_json::Value>,
}

/// CPU-based ONNX CLIP Runner with f16 quantization and graceful degradation.
pub struct ClipEngine {
    config: ClipConfig,
    sessions: Arc<Mutex<Option<LoadedClipSessions>>>,
}

impl ClipEngine {
    pub fn new(config: ClipConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(None)),
        }
    }

    /// Check whether CLIP model files are available on disk.
    pub fn is_available(&self) -> bool {
        self.config.visual_model_path.exists()
            && self.config.text_model_path.exists()
            && self.config.tokenizer_path.exists()
    }

    /// Load or retrieve cached ONNX sessions for visual and text encoders.
    fn get_or_load_sessions(
        &self,
    ) -> Result<parking_lot::MutexGuard<'_, Option<LoadedClipSessions>>, String> {
        let mut guard = self.sessions.lock();
        if guard.is_none() {
            let visual_session =
                load_session(&self.config.visual_model_path, self.config.num_threads);
            let text_session = load_session(&self.config.text_model_path, self.config.num_threads);
            let vocab = fs::read_to_string(&self.config.tokenizer_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok());

            *guard = Some(LoadedClipSessions {
                visual_session,
                text_session,
                vocab,
            });
        }
        Ok(guard)
    }

    /// Generate visual embedding for an image.
    pub fn embed_image(&self, img: &image::DynamicImage) -> Result<Option<Vec<f16>>, String> {
        if !self.is_available() {
            debug!("CLIP model not available, returning None");
            return Ok(None);
        }

        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);

        let resized = img.resize_exact(224, 224, image::imageops::FilterType::Triangle);
        let rgb_img = resized.to_rgb8();

        let mut sessions_guard = self.get_or_load_sessions()?;
        let sessions = sessions_guard
            .as_mut()
            .ok_or("CLIP sessions not initialized")?;

        if start_time.elapsed() >= timeout {
            warn!("CLIP timed out");
            return Ok(None);
        }

        let mean = [0.481_454_66_f32, 0.457_827_5_f32, 0.408_210_73_f32];
        let std = [0.268_629_55_f32, 0.261_302_6_f32, 0.275_777_1_f32];

        let mut visual_input = Vec::with_capacity(3 * 224 * 224);
        for c in 0..3 {
            for y in 0..224 {
                for x in 0..224 {
                    let pixel = rgb_img.get_pixel(x, y);
                    let val = (pixel[c] as f32 / 255.0 - mean[c]) / std[c];
                    visual_input.push(val);
                }
            }
        }

        let shape = vec![1, 3, 224_i64, 224_i64];
        let input_tensor = Tensor::from_array((shape, visual_input))
            .map_err(|e| format!("failed to create visual tensor: {e}"))?;

        let visual_inputs = ort::inputs!["x" => input_tensor];
        let vis_sess = sessions
            .visual_session
            .as_mut()
            .ok_or("Visual session not loaded")?;

        if let Some(embedding) = vis_sess
            .run(visual_inputs)
            .ok()
            .and_then(extract_l2_normalized_embedding)
        {
            return Ok(Some(embedding));
        }

        warn!("CLIP visual encoding failed");
        Ok(None)
    }

    /// Generate text embedding for a query.
    pub fn embed_text(&self, text: &str) -> Result<Option<Vec<f16>>, String> {
        if !self.is_available() {
            debug!("CLIP model not available, returning None");
            return Ok(None);
        }

        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);

        let tokens = self.simple_tokenize(text);

        let mut sessions_guard = self.get_or_load_sessions()?;
        let sessions = sessions_guard
            .as_mut()
            .ok_or("CLIP sessions not initialized")?;

        if start_time.elapsed() >= timeout {
            warn!("CLIP timed out");
            return Ok(None);
        }

        let mut text_input = Vec::with_capacity(77);
        text_input.push(49406_i64);

        for token in tokens.into_iter().take(75) {
            let token_id = (self.simple_hash_to_id(&token) % 49408) as i64;
            text_input.push(token_id);
        }

        text_input.push(49407_i64);

        while text_input.len() < 77 {
            text_input.push(0_i64);
        }

        let shape = vec![1, 77_i64];
        let input_tensor = Tensor::from_array((shape, text_input))
            .map_err(|e| format!("failed to create text tensor: {e}"))?;

        let text_inputs = ort::inputs!["x" => input_tensor];
        let txt_sess = sessions
            .text_session
            .as_mut()
            .ok_or("Text session not loaded")?;

        if let Some(embedding) = txt_sess
            .run(text_inputs)
            .ok()
            .and_then(extract_l2_normalized_embedding)
        {
            return Ok(Some(embedding));
        }

        warn!("CLIP text encoding failed");
        Ok(None)
    }

    /// Simple whitespace tokenizer for demonstration.
    fn simple_tokenize(&self, text: &str) -> Vec<String> {
        text.split_whitespace()
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Simple hash-based token to ID mapping for demonstration.
    fn simple_hash_to_id(&self, token: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn test_clip_fallback_when_models_missing() {
        let tmp = tempdir().unwrap();
        let engine = ClipEngine::new(ClipConfig::new(tmp.path()));
        assert!(!engine.is_available());

        let img = DynamicImage::ImageRgb8(RgbImage::new(224, 224));
        let result = engine.embed_image(&img);
        assert!(result.is_ok());
        let embedding = result.unwrap();
        assert!(embedding.is_none());

        let text_result = engine.embed_text("test query");
        assert!(text_result.is_ok());
        let text_embedding = text_result.unwrap();
        assert!(text_embedding.is_none());
    }

    #[test]
    fn test_simple_tokenize() {
        let tmp = tempdir().unwrap();
        let engine = ClipEngine::new(ClipConfig::new(tmp.path()));
        let tokens = engine.simple_tokenize("Hello World TEST");
        assert_eq!(tokens, vec!["hello", "world", "test"]);
    }
}
