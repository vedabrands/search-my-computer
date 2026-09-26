use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tracing::debug;

/// Standard display resolutions commonly found in screenshots (width, height) regardless of orientation.
const SCREENSHOT_RESOLUTIONS: &[(u32, u32)] = &[
    (1920, 1080), // 1080p FHD
    (2560, 1440), // 1440p QHD / 2K
    (3840, 2160), // 4K UHD
    (2880, 1800), // Retina 15"/16" MacBook
    (3024, 1964), // M1/M2/M3 Pro 14" MacBook
    (3456, 2234), // M1/M2/M3 Max 16" MacBook
    (2560, 1600), // 13" MacBook / Surface
    (1366, 768),  // Budget laptops
    (1440, 900),  // Older MacBooks / widescreen
    (1600, 900),  // HD+
    (1280, 720),  // 720p HD
    (1280, 800),  // WXGA
    (1536, 864),  // Surface Go / scaled laptops
    (1920, 1200), // 16:10 FHD
    (2256, 1504), // Surface Laptop (3:2)
    (3000, 2000), // Surface Book (3:2)
    (3840, 2400), // 16:10 4K
    (5120, 2880), // 5K iMac / Studio Display
    // Mobile screen resolutions
    (1170, 2532), // iPhone 12/13/14
    (1179, 2556), // iPhone 14/15 Pro
    (1290, 2796), // iPhone 14/15 Pro Max
    (1080, 2400), // Typical Android FHD+
    (1080, 2340), // Typical Android
    (1440, 3120), // Typical Android QHD+
];

/// Keywords in filename or parent path indicative of a screenshot or screen recording.
const SCREENSHOT_KEYWORDS: &[&str] = &[
    "screenshot",
    "screen shot",
    "screen_shot",
    "screengrab",
    "screen grab",
    "screen_grab",
    "capture",
    "snip",
    "snipping",
    "printscreen",
    "prtscn",
    "cleanshot",
    "lightshot",
    "greenshot",
    "gyazo",
    "sharex",
];

/// Image metadata and screenshot classification information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub exif_date: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub is_screenshot: bool,
}

impl ImageMetadata {
    /// Extract dimensions, EXIF data, and determine screenshot status from an image file.
    pub fn extract_from_path(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("failed to open image: {e}"))?;
        let mut reader = BufReader::new(file);

        // 1. Read image dimensions and format using the image crate's reader
        let img_reader = image::ImageReader::open(path)
            .map_err(|e| format!("failed to open image reader: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("failed to guess format: {e}"))?;

        let format = img_reader
            .format()
            .map(|f| format!("{:?}", f).to_lowercase())
            .unwrap_or_else(|| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("unknown")
                    .to_lowercase()
            });

        let (width, height) = img_reader
            .into_dimensions()
            .map_err(|e| format!("failed to decode image dimensions: {e}"))?;

        // 2. Read EXIF metadata if present (using kamadak-exif)
        let (exif_date, camera_make, camera_model) = Self::read_exif(&mut reader);

        // 3. Apply Screenshot Heuristics
        let is_screenshot = Self::detect_screenshot(
            path,
            width,
            height,
            camera_make.as_deref(),
            camera_model.as_deref(),
        );

        Ok(Self {
            width,
            height,
            format,
            exif_date,
            camera_make,
            camera_model,
            is_screenshot,
        })
    }

    /// Read EXIF tags: DateTimeOriginal, Make, Model
    fn read_exif(reader: &mut BufReader<File>) -> (Option<String>, Option<String>, Option<String>) {
        let mut exif_date = None;
        let mut camera_make = None;
        let mut camera_model = None;

        if let Ok(exif) = exif::Reader::new().read_from_container(reader) {
            // EXIF Date Time Original
            if let Some(field) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
                let s = field.display_value().to_string();
                if !s.trim().is_empty() {
                    exif_date = Some(s.trim().to_string());
                }
            } else if let Some(field) = exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY) {
                let s = field.display_value().to_string();
                if !s.trim().is_empty() {
                    exif_date = Some(s.trim().to_string());
                }
            }

            // Camera Make
            if let Some(field) = exif.get_field(exif::Tag::Make, exif::In::PRIMARY) {
                let s = field.display_value().to_string();
                let clean = s.trim().trim_matches('"');
                if !clean.is_empty() {
                    camera_make = Some(clean.to_string());
                }
            }

            // Camera Model
            if let Some(field) = exif.get_field(exif::Tag::Model, exif::In::PRIMARY) {
                let s = field.display_value().to_string();
                let clean = s.trim().trim_matches('"');
                if !clean.is_empty() {
                    camera_model = Some(clean.to_string());
                }
            }
        }

        (exif_date, camera_make, camera_model)
    }

    /// Determine if an image is likely a screenshot based on:
    /// 1. Filename or path keywords ("Screenshot", "Screen Shot", "Capture", "Snip")
    /// 2. Resolution matching common screen / mobile display resolutions
    /// 3. Absence of camera EXIF data (no camera make/model)
    pub fn detect_screenshot(
        path: &Path,
        width: u32,
        height: u32,
        camera_make: Option<&str>,
        camera_model: Option<&str>,
    ) -> bool {
        // If image clearly has a camera make/model, it's a camera photo, not a screenshot
        if camera_make.is_some() || camera_model.is_some() {
            return false;
        }

        let path_str = path.to_string_lossy().to_lowercase();

        // 1. Check path/filename keywords
        let keyword_match = SCREENSHOT_KEYWORDS.iter().any(|&kw| path_str.contains(kw));

        if keyword_match {
            debug!(path = %path.display(), "screenshot detected by filename/path keyword");
            return true;
        }

        // 2. Check resolution against known display resolution profiles
        let resolution_match = SCREENSHOT_RESOLUTIONS
            .iter()
            .any(|&(w, h)| (width == w && height == h) || (width == h && height == w));

        // 3. Aspect ratio check for typical modern computer screens (16:9, 16:10, 3:2, 21:9)
        let is_display_aspect = if width > 0 && height > 0 {
            let ratio = width as f32 / height as f32;
            let inv_ratio = height as f32 / width as f32;
            let r = if ratio >= 1.0 { ratio } else { inv_ratio };

            // 16:9 (~1.777), 16:10 (1.60), 3:2 (1.50), 21:9 (~2.333), 4:3 (1.333)
            ((r - 1.777).abs() < 0.02)
                || ((r - 1.600).abs() < 0.02)
                || ((r - 1.500).abs() < 0.02)
                || ((r - 2.333).abs() < 0.05)
        } else {
            false
        };

        // If resolution matches exactly and no camera EXIF exists, highly likely screenshot
        if resolution_match {
            debug!(path = %path.display(), width, height, "screenshot detected by exact display resolution match");
            return true;
        }

        // PNG / WebP images with typical display aspect ratio and standard sizes (> 720p)
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();

        if (ext == "png" || ext == "webp") && is_display_aspect && width >= 1280 && height >= 720 {
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_screenshot_detection_by_keyword() {
        let p1 = PathBuf::from("C:\\Users\\dev\\Pictures\\Screenshot 2026-09-24 103015.png");
        assert!(ImageMetadata::detect_screenshot(&p1, 800, 600, None, None));

        let p2 = PathBuf::from("C:\\Users\\dev\\Desktop\\CleanShot 2026-01-01 at 12.00.00.png");
        assert!(ImageMetadata::detect_screenshot(&p2, 1024, 768, None, None));

        let p3 = PathBuf::from("C:\\Users\\dev\\Documents\\snip_receipt.jpg");
        assert!(ImageMetadata::detect_screenshot(&p3, 500, 500, None, None));
    }

    #[test]
    fn test_screenshot_detection_by_resolution() {
        let p = PathBuf::from("C:\\Users\\dev\\Downloads\\image1.png");
        assert!(ImageMetadata::detect_screenshot(&p, 1920, 1080, None, None));
        assert!(ImageMetadata::detect_screenshot(&p, 2560, 1440, None, None));
        assert!(ImageMetadata::detect_screenshot(&p, 2880, 1800, None, None));
    }

    #[test]
    fn test_camera_photo_not_screenshot() {
        let p = PathBuf::from("C:\\Users\\dev\\Pictures\\Screenshot 2026.jpg");
        // Even if name contains screenshot, camera make/model takes precedence as real photo
        assert!(!ImageMetadata::detect_screenshot(
            &p,
            1920,
            1080,
            Some("Canon"),
            Some("EOS R5")
        ));
    }
}
