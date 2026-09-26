use image::ImageFormat;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Default maximum dimension for generated thumbnails.
pub const THUMBNAIL_MAX_DIMENSION: u32 = 256;

/// Helper to generate and cache image thumbnails in the app-data thumbnails directory.
pub struct ThumbnailManager {
    thumbnail_dir: PathBuf,
}

impl ThumbnailManager {
    /// Create a new thumbnail manager pointing to the given cache directory.
    pub fn new(thumbnail_dir: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&thumbnail_dir) {
            warn!(dir = %thumbnail_dir.display(), error = %e, "failed to create thumbnail directory");
        }
        Self { thumbnail_dir }
    }

    /// Compute the thumbnail file path for a given image source path and content hash or mtime.
    pub fn thumbnail_path(&self, source_path: &Path, content_hash: Option<&str>) -> PathBuf {
        let key = if let Some(hash) = content_hash {
            hash.to_string()
        } else {
            let mut hasher = Sha256::new();
            hasher.update(source_path.to_string_lossy().as_bytes());
            format!("{:x}", hasher.finalize())
        };

        self.thumbnail_dir.join(format!("{}.jpg", key))
    }

    /// Check if a thumbnail already exists on disk.
    pub fn has_thumbnail(&self, source_path: &Path, content_hash: Option<&str>) -> bool {
        self.thumbnail_path(source_path, content_hash).exists()
    }

    /// Generate and save a thumbnail for the image at `source_path`.
    /// Returns the PathBuf to the saved thumbnail.
    pub fn get_or_create_thumbnail(
        &self,
        source_path: &Path,
        content_hash: Option<&str>,
        max_dimension: Option<u32>,
    ) -> Result<PathBuf, String> {
        let dest_path = self.thumbnail_path(source_path, content_hash);

        // Fast path: thumbnail already generated
        if dest_path.exists() {
            return Ok(dest_path);
        }

        // Open and decode image
        let img = image::ImageReader::open(source_path)
            .map_err(|e| format!("failed to open image: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("failed to guess format: {e}"))?
            .decode()
            .map_err(|e| format!("failed to decode image: {e}"))?;

        let max_dim = max_dimension.unwrap_or(THUMBNAIL_MAX_DIMENSION);
        let thumbnail = img.thumbnail(max_dim, max_dim);

        // Ensure parent directory exists
        if let Some(parent) = dest_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Save as JPEG (quality 85)
        thumbnail
            .save_with_format(&dest_path, ImageFormat::Jpeg)
            .map_err(|e| format!("failed to save thumbnail: {e}"))?;

        debug!(source = %source_path.display(), thumb = %dest_path.display(), "thumbnail generated");
        Ok(dest_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn test_thumbnail_generation_and_caching() {
        let tmp = tempdir().unwrap();
        let thumb_dir = tmp.path().join("thumbnails");
        let manager = ThumbnailManager::new(thumb_dir.clone());

        // Create a test image 800x600
        let img_path = tmp.path().join("test_img.png");
        let mut img = RgbImage::new(800, 600);
        for pixel in img.pixels_mut() {
            *pixel = Rgb([120, 150, 200]);
        }
        img.save(&img_path).unwrap();

        assert!(!manager.has_thumbnail(&img_path, None));

        let thumb_path = manager
            .get_or_create_thumbnail(&img_path, None, Some(128))
            .unwrap();

        assert!(thumb_path.exists());
        assert!(manager.has_thumbnail(&img_path, None));

        // Verify dimensions of the saved thumbnail
        let loaded_thumb = image::open(&thumb_path).unwrap();
        assert!(loaded_thumb.width() <= 128);
        assert!(loaded_thumb.height() <= 128);
    }
}
