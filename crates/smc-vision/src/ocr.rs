use image::{DynamicImage, GenericImageView, ImageReader};
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use smc_extract::extractor::TextBlock;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Default maximum dimension for OCR image processing (downscales larger images).
pub const OCR_MAX_DIMENSION: u32 = 960;
/// Default timeout per image for OCR in milliseconds.
pub const OCR_DEFAULT_TIMEOUT_MS: u64 = 2000;

/// Configuration for the local CPU ONNX OCR engine.
#[derive(Debug, Clone)]
pub struct OcrConfig {
    pub det_model_path: PathBuf,
    pub rec_model_path: PathBuf,
    pub keys_path: PathBuf,
    pub timeout_ms: u64,
    pub max_dimension: u32,
    pub num_threads: usize,
}

impl Default for OcrConfig {
    fn default() -> Self {
        let models_dir = PathBuf::from("models").join("ocr");
        Self {
            det_model_path: models_dir.join("det.onnx"),
            rec_model_path: models_dir.join("rec.onnx"),
            keys_path: models_dir.join("keys.txt"),
            timeout_ms: OCR_DEFAULT_TIMEOUT_MS,
            max_dimension: OCR_MAX_DIMENSION,
            num_threads: 2,
        }
    }
}

impl OcrConfig {
    pub fn new(models_dir: &Path) -> Self {
        let dir = models_dir.join("ocr");
        Self {
            det_model_path: dir.join("det.onnx"),
            rec_model_path: dir.join("rec.onnx"),
            keys_path: dir.join("keys.txt"),
            timeout_ms: OCR_DEFAULT_TIMEOUT_MS,
            max_dimension: OCR_MAX_DIMENSION,
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

/// Loaded ONNX sessions and character dictionary.
struct LoadedOcrSessions {
    det_session: Option<Session>,
    rec_session: Option<Session>,
    char_dict: Vec<char>,
}

/// CPU-based ONNX OCR Runner with downscaling, CTC decoding, and per-image timeout.
pub struct OcrEngine {
    config: OcrConfig,
    sessions: Arc<Mutex<Option<LoadedOcrSessions>>>,
}

impl OcrEngine {
    pub fn new(config: OcrConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(None)),
        }
    }

    /// Check whether OCR model files are available on disk.
    pub fn is_available(&self) -> bool {
        self.config.det_model_path.exists() && self.config.rec_model_path.exists()
    }

    /// Load or retrieve cached ONNX sessions for detection and recognition.
    fn get_or_load_sessions(
        &self,
    ) -> Result<parking_lot::MutexGuard<'_, Option<LoadedOcrSessions>>, String> {
        let mut guard = self.sessions.lock();
        if guard.is_none() {
            let det_session = load_session(&self.config.det_model_path, self.config.num_threads);
            let rec_session = load_session(&self.config.rec_model_path, self.config.num_threads);
            let mut char_dict = Vec::new();

            if let Ok(content) = fs::read_to_string(&self.config.keys_path) {
                char_dict = content
                    .chars()
                    .filter(|&c| c != '\r' && c != '\n')
                    .collect();
            }

            *guard = Some(LoadedOcrSessions {
                det_session,
                rec_session,
                char_dict,
            });
        }
        Ok(guard)
    }

    /// Downscale an image proportionally if its dimensions exceed `max_dimension`.
    pub fn downscale_image(&self, img: &DynamicImage) -> DynamicImage {
        let (w, h) = img.dimensions();
        let max_dim = self.config.max_dimension;
        if w > max_dim || h > max_dim {
            let ratio = (max_dim as f32) / (w.max(h) as f32);
            let new_w = ((w as f32 * ratio).round() as u32).max(32);
            let new_h = ((h as f32 * ratio).round() as u32).max(32);
            img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle)
        } else {
            img.clone()
        }
    }

    /// Run OCR on an image file path with enforced timeout.
    pub fn extract_from_path(&self, path: &Path) -> Result<Vec<TextBlock>, String> {
        let img = ImageReader::open(path)
            .map_err(|e| format!("failed to open image: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("failed to guess format: {e}"))?
            .decode()
            .map_err(|e| format!("failed to decode image: {e}"))?;

        self.extract_from_image(&img, None)
    }

    /// Run OCR on a decoded image with enforced timeout.
    pub fn extract_from_image(
        &self,
        img: &DynamicImage,
        page_num: Option<usize>,
    ) -> Result<Vec<TextBlock>, String> {
        let start_time = Instant::now();
        let timeout = Duration::from_millis(self.config.timeout_ms);

        let downscaled = self.downscale_image(img);
        let rgb_img = downscaled.to_rgb8();
        let (width, height) = rgb_img.dimensions();

        let mut sessions_guard = self.get_or_load_sessions()?;
        let sessions = sessions_guard
            .as_mut()
            .ok_or("OCR sessions not initialized")?;

        let mut text_lines = Vec::new();

        // 1. If models are present, run ONNX inference
        if let (Some(det), Some(rec)) = (&mut sessions.det_session, &mut sessions.rec_session) {
            // Check timeout before detection
            if start_time.elapsed() >= timeout {
                warn!("OCR timed out before detection");
                return Ok(Vec::new());
            }

            let det_w = width.div_ceil(32) * 32;
            let det_h = height.div_ceil(32) * 32;

            if let Ok(regions) = run_detection(det, &rgb_img, det_w, det_h) {
                debug!(count = regions.len(), "detected text regions");

                for (idx, (rx, ry, rw, rh)) in regions.into_iter().enumerate() {
                    if start_time.elapsed() >= timeout {
                        warn!(
                            lines_processed = idx,
                            "OCR reached timeout, returning partial results"
                        );
                        break;
                    }

                    // Crop and recognize text line
                    let crop_x = (rx as f32 * (width as f32 / det_w as f32)).round() as u32;
                    let crop_y = (ry as f32 * (height as f32 / det_h as f32)).round() as u32;
                    let crop_w = (rw as f32 * (width as f32 / det_w as f32)).round() as u32;
                    let crop_h = (rh as f32 * (height as f32 / det_h as f32)).round() as u32;

                    if crop_w > 8 && crop_h > 8 {
                        let cropped = image::imageops::crop_imm(
                            &rgb_img,
                            crop_x.min(width - 1),
                            crop_y.min(height - 1),
                            crop_w.min(width - crop_x),
                            crop_h.min(height - crop_y),
                        )
                        .to_image();

                        if let Some(recognized_text) =
                            recognize_line(rec, &sessions.char_dict, &cropped)
                        {
                            let trimmed = recognized_text.trim();
                            if !trimmed.is_empty() {
                                text_lines.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Convert recognized lines into TextBlock objects
        let mut blocks = Vec::new();
        let mut offset = 0;
        let section = page_num
            .map(|p| format!("Page {}", p))
            .or_else(|| Some("OCR".to_string()));

        for line in text_lines {
            let len = line.len();
            let start = offset;
            let end = offset + len;
            offset = end + 2;

            blocks.push(TextBlock::new(line, page_num, section.clone(), start, end));
        }

        Ok(blocks)
    }
}

/// Run DBNet text detection on an RGB image.
fn run_detection(
    det: &mut Session,
    rgb_img: &image::RgbImage,
    det_w: u32,
    det_h: u32,
) -> Result<Vec<(u32, u32, u32, u32)>, String> {
    let resized_det =
        image::imageops::resize(rgb_img, det_w, det_h, image::imageops::FilterType::Triangle);

    let mut det_input = Vec::with_capacity((3 * det_w * det_h) as usize);
    let mean = [0.485f32, 0.456f32, 0.406f32];
    let std = [0.229f32, 0.224f32, 0.225f32];

    for c in 0..3 {
        for y in 0..det_h {
            for x in 0..det_w {
                let pixel = resized_det.get_pixel(x, y);
                let val = (pixel[c] as f32 / 255.0 - mean[c]) / std[c];
                det_input.push(val);
            }
        }
    }

    let shape = vec![1, 3, det_h as i64, det_w as i64];
    let input_tensor = Tensor::from_array((shape, det_input))
        .map_err(|e| format!("failed to create det tensor: {e}"))?;

    let det_inputs = ort::inputs!["x" => input_tensor];
    let outputs = det
        .run(det_inputs)
        .map_err(|e| format!("det run failed: {e}"))?;
    let (_name, val) = outputs.into_iter().next().ok_or("no det output")?;
    let (_out_shape, out_data) = val
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("extract det tensor failed: {e}"))?;

    Ok(find_text_regions(out_data, det_w, det_h, 0.3))
}

/// Helper to threshold segmentation probabilities into bounding boxes.
fn find_text_regions(
    prob_map: &[f32],
    w: u32,
    h: u32,
    threshold: f32,
) -> Vec<(u32, u32, u32, u32)> {
    let mut regions = Vec::new();
    let min_area = 64;

    // Simple connected component / bounding box extraction
    let mut visited = vec![false; (w * h) as usize];

    for y in (0..h).step_by(4) {
        for x in (0..w).step_by(4) {
            let idx = (y * w + x) as usize;
            if idx < prob_map.len() && prob_map[idx] > threshold && !visited[idx] {
                // Expand box
                let mut min_x = x;
                let mut max_x = x;
                let mut min_y = y;
                let mut max_y = y;

                let mut stack = vec![(x, y)];
                visited[idx] = true;

                while let Some((cx, cy)) = stack.pop() {
                    min_x = min_x.min(cx);
                    max_x = max_x.max(cx);
                    min_y = min_y.min(cy);
                    max_y = max_y.max(cy);

                    // Check 4-neighbors with stride 4
                    let neighbors = [
                        (cx.saturating_sub(4), cy),
                        ((cx + 4).min(w - 1), cy),
                        (cx, cy.saturating_sub(4)),
                        (cx, (cy + 4).min(h - 1)),
                    ];

                    for &(nx, ny) in &neighbors {
                        let n_idx = (ny * w + nx) as usize;
                        if n_idx < prob_map.len() && prob_map[n_idx] > threshold && !visited[n_idx]
                        {
                            visited[n_idx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }

                let bw = max_x.saturating_sub(min_x) + 4;
                let bh = max_y.saturating_sub(min_y) + 4;
                if bw * bh >= min_area {
                    regions.push((min_x, min_y, bw, bh));
                }
            }
        }
    }

    regions
}

/// Run recognition ONNX model on a cropped text line image and CTC decode.
fn recognize_line(
    rec_session: &mut Session,
    char_dict: &[char],
    crop: &image::RgbImage,
) -> Option<String> {
    let (cw, ch) = crop.dimensions();
    if cw < 4 || ch < 4 {
        return None;
    }

    // Standard OCR recognition input height = 48
    let target_h = 48u32;
    let target_w = (((cw as f32 / ch as f32) * target_h as f32).round() as u32).clamp(16, 640);

    let resized = image::imageops::resize(
        crop,
        target_w,
        target_h,
        image::imageops::FilterType::Triangle,
    );

    let mut rec_input = Vec::with_capacity((3 * target_h * target_w) as usize);
    for c in 0..3 {
        for y in 0..target_h {
            for x in 0..target_w {
                let pixel = resized.get_pixel(x, y);
                // Normalized (x / 255 - 0.5) / 0.5
                let val = (pixel[c] as f32 / 255.0 - 0.5) / 0.5;
                rec_input.push(val);
            }
        }
    }

    let shape = vec![1, 3, target_h as i64, target_w as i64];
    let input_tensor = Tensor::from_array((shape, rec_input)).ok()?;
    let rec_inputs = ort::inputs!["x" => input_tensor];

    let outputs = rec_session.run(rec_inputs).ok()?;
    let (_name, val) = outputs.into_iter().next()?;
    let (out_shape, out_data) = val.try_extract_tensor::<f32>().ok()?;

    // CTC Greedy Decoder: [batch, time_steps, num_classes]
    if out_shape.len() >= 3 {
        let time_steps = out_shape[1] as usize;
        let num_classes = out_shape[2] as usize;

        let mut prev_class = 0;
        let mut text = String::new();

        for t in 0..time_steps {
            let step_slice = &out_data[t * num_classes..(t + 1) * num_classes];
            let mut best_class = 0;
            let mut best_score = f32::NEG_INFINITY;

            for (c, &score) in step_slice.iter().enumerate() {
                if score > best_score {
                    best_score = score;
                    best_class = c;
                }
            }

            // Index 0 is CTC blank token
            if best_class != 0 && best_class != prev_class {
                if best_class - 1 < char_dict.len() {
                    text.push(char_dict[best_class - 1]);
                } else {
                    // Fallback character
                    text.push(' ');
                }
            }
            prev_class = best_class;
        }

        return Some(text);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;
    use tempfile::tempdir;

    #[test]
    fn test_ocr_downscaling() {
        let engine = OcrEngine::new(OcrConfig {
            max_dimension: 500,
            ..Default::default()
        });

        // 2000 x 1000 image
        let img = DynamicImage::ImageRgb8(RgbImage::new(2000, 1000));
        let downscaled = engine.downscale_image(&img);

        let (w, h) = downscaled.dimensions();
        assert!(w <= 500);
        assert!(h <= 500);
        assert_eq!(w, 500);
        assert_eq!(h, 250);
    }

    #[test]
    fn test_ocr_fallback_when_models_missing() {
        let tmp = tempdir().unwrap();
        let engine = OcrEngine::new(OcrConfig::new(tmp.path()));
        assert!(!engine.is_available());

        let img = DynamicImage::ImageRgb8(RgbImage::new(200, 100));
        let result = engine.extract_from_image(&img, Some(1));
        assert!(result.is_ok());
        let blocks = result.unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_find_text_regions() {
        let mut prob_map = vec![0.0f32; 100 * 100];
        // Draw a block with high probability (20..40, 20..40)
        for y in 20..40 {
            for x in 20..40 {
                prob_map[y * 100 + x] = 0.9;
            }
        }

        let regions = find_text_regions(&prob_map, 100, 100, 0.5);
        assert_eq!(regions.len(), 1);
        let (rx, ry, rw, rh) = regions[0];
        assert!(rx <= 20 && ry <= 20);
        assert!(rw >= 20 && rh >= 20);
    }
}
