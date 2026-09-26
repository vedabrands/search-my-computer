use crate::clip::ClipEngine;
use crate::metadata::ImageMetadata;
use crate::ocr::OcrEngine;
use crate::qr::{DecodedBarcode, decode_barcodes};
use crate::thumbnail::ThumbnailManager;
use half::f16;
use rusqlite::{Connection, OptionalExtension, params};
use smc_core::db::Database;
use smc_core::schema::{TABLE_CHUNKS, TABLE_IMAGE_METADATA, TABLE_IMAGE_TAGS, TABLE_IMAGE_VECTORS};
use smc_extract::extractor::TextBlock;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Structured output of the full vision processing pipeline for an image.
#[derive(Debug, Clone)]
pub struct VisionResult {
    pub file_id: i64,
    pub metadata: ImageMetadata,
    pub thumbnail_path: Option<PathBuf>,
    pub barcodes: Vec<DecodedBarcode>,
    pub ocr_blocks: Vec<TextBlock>,
    pub visual_embedding: Option<Vec<f16>>,
}

/// Vision pipeline orchestrator coordinating metadata, thumbnailing, QR, OCR, and CLIP.
pub struct VisionPipeline {
    thumbnail_manager: ThumbnailManager,
    ocr_engine: OcrEngine,
    clip_engine: ClipEngine,
}

impl VisionPipeline {
    pub fn new(thumbnail_dir: PathBuf, ocr_engine: OcrEngine, clip_engine: ClipEngine) -> Self {
        Self {
            thumbnail_manager: ThumbnailManager::new(thumbnail_dir),
            ocr_engine,
            clip_engine,
        }
    }

    pub fn clip_engine(&self) -> &ClipEngine {
        &self.clip_engine
    }

    pub fn ocr_engine(&self) -> &OcrEngine {
        &self.ocr_engine
    }

    pub fn thumbnail_manager(&self) -> &ThumbnailManager {
        &self.thumbnail_manager
    }

    /// Process a single image file through all active pipeline stages.
    pub fn process_file(
        &self,
        file_id: i64,
        path: &Path,
        content_hash: Option<&str>,
    ) -> Result<VisionResult, String> {
        // 1. Extract metadata & screenshot heuristics
        let metadata = ImageMetadata::extract_from_path(path)?;

        // 2. Generate or fetch cached thumbnail
        let thumbnail_path = self
            .thumbnail_manager
            .get_or_create_thumbnail(path, content_hash, None)
            .ok();

        // 3. Decode barcodes & QR codes
        let barcodes = decode_barcodes(path).unwrap_or_else(|e| {
            debug!(path = %path.display(), error = %e, "barcode decoding skipped/failed");
            Vec::new()
        });

        // 4. Run OCR text extraction if models present or fallback
        let ocr_blocks = self.ocr_engine.extract_from_path(path).unwrap_or_else(|e| {
            debug!(path = %path.display(), error = %e, "OCR skipped/failed");
            Vec::new()
        });

        // 5. Run visual CLIP embedding if models present
        let visual_embedding = if self.clip_engine.is_available() {
            if let Ok(img) = image::open(path) {
                self.clip_engine.embed_image(&img).unwrap_or(None)
            } else {
                None
            }
        } else {
            None
        };

        Ok(VisionResult {
            file_id,
            metadata,
            thumbnail_path,
            barcodes,
            ocr_blocks,
            visual_embedding,
        })
    }

    /// Persist the extracted vision results into SQLite.
    pub fn persist_results(
        &self,
        conn: &mut Connection,
        result: &VisionResult,
    ) -> Result<(), rusqlite::Error> {
        let tx = conn.transaction()?;

        let now = chrono::Utc::now().to_rfc3339();
        let has_qr = !result.barcodes.is_empty();
        let qr_count = result.barcodes.len() as i64;

        // 1. Insert/replace image metadata
        let metadata_sql = format!(
            "INSERT OR REPLACE INTO {} (
                file_id, width, height, format, exif_date, camera_make, camera_model, is_screenshot, has_qr, qr_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            TABLE_IMAGE_METADATA
        );

        tx.execute(
            &metadata_sql,
            params![
                result.file_id,
                result.metadata.width,
                result.metadata.height,
                result.metadata.format,
                result.metadata.exif_date,
                result.metadata.camera_make,
                result.metadata.camera_model,
                result.metadata.is_screenshot,
                has_qr,
                qr_count,
            ],
        )?;

        // 2. Clear previous tags for this file and insert fresh tags
        let delete_tags_sql = format!("DELETE FROM {} WHERE file_id = ?1", TABLE_IMAGE_TAGS);
        tx.execute(&delete_tags_sql, params![result.file_id])?;

        let insert_tag_sql = format!(
            "INSERT INTO {} (file_id, tag, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
            TABLE_IMAGE_TAGS
        );

        // Screenshot tag
        if result.metadata.is_screenshot {
            tx.execute(
                &insert_tag_sql,
                params![
                    result.file_id,
                    "type:screenshot",
                    Option::<String>::None,
                    now
                ],
            )?;
        } else if result.metadata.camera_make.is_some() || result.metadata.camera_model.is_some() {
            tx.execute(
                &insert_tag_sql,
                params![result.file_id, "type:photo", Option::<String>::None, now],
            )?;
        }

        // Barcode / QR tags
        for barcode in &result.barcodes {
            tx.execute(
                &insert_tag_sql,
                params![result.file_id, barcode.tag, barcode.masked_payload, now],
            )?;
        }

        // 3. Insert visual embeddings if present
        if let Some(ref embedding) = result.visual_embedding {
            let delete_vec_sql = format!("DELETE FROM {} WHERE file_id = ?1", TABLE_IMAGE_VECTORS);
            tx.execute(&delete_vec_sql, params![result.file_id])?;

            // Convert f16 slice to raw byte slice
            let byte_slice: &[u8] = unsafe {
                std::slice::from_raw_parts(
                    embedding.as_ptr() as *const u8,
                    embedding.len() * std::mem::size_of::<f16>(),
                )
            };

            let insert_vec_sql = format!(
                "INSERT INTO {} (file_id, model_id, dims, vector) VALUES (?1, ?2, ?3, ?4)",
                TABLE_IMAGE_VECTORS
            );

            tx.execute(
                &insert_vec_sql,
                params![
                    result.file_id,
                    "clip-vit-b32",
                    embedding.len() as i64,
                    byte_slice,
                ],
            )?;
        }

        // 4. Insert OCR text blocks as chunks so they flow into FTS and semantic embeddings
        if !result.ocr_blocks.is_empty() {
            let delete_chunks_sql = format!("DELETE FROM {} WHERE file_id = ?1", TABLE_CHUNKS);
            tx.execute(&delete_chunks_sql, params![result.file_id])?;

            let insert_chunk_sql = format!(
                "INSERT INTO {} (file_id, ordinal, text, page, section, symbol, start, end) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                TABLE_CHUNKS
            );

            for (i, block) in result.ocr_blocks.iter().enumerate() {
                tx.execute(
                    &insert_chunk_sql,
                    params![
                        result.file_id,
                        i as i64,
                        block.text,
                        block.page.map(|p| p as i64),
                        block.section,
                        Option::<String>::None,
                        block.start_offset as i64,
                        block.end_offset as i64,
                    ],
                )?;
            }

            // Enqueue embedding job for OCR text
            let _ = tx.execute(
                "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                 SELECT ?1, ?2, 2, ?3, ?4
                 WHERE NOT EXISTS (
                     SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                 )",
                params![
                    smc_core::schema::job_kind::EMBED,
                    result.file_id,
                    smc_core::schema::job_state::PENDING,
                    now,
                ],
            );
        }

        tx.commit()?;
        Ok(())
    }
}

/// Process a vision job for a specific file_id, updating SQLite.
pub fn process_vision_job(
    db: &Database,
    file_id: i64,
    pipeline: &VisionPipeline,
) -> Result<(), String> {
    let conn = db.reader().map_err(|e| format!("db pool error: {e}"))?;

    let file_info: Option<(String, String)> = conn
        .query_row(
            "SELECT path, status FROM files WHERE id = ?1",
            rusqlite::params![file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("db query error: {e}"))?;

    let (path_str, status) = match file_info {
        Some(info) => info,
        None => return Ok(()),
    };

    if status == "deleted" {
        return Ok(());
    }

    let path = Path::new(&path_str);
    if !path.exists() {
        let writer = db.writer();
        let _ = writer.execute(
            "UPDATE files SET status = 'error', error = 'image not found on disk' WHERE id = ?1",
            rusqlite::params![file_id],
        );
        return Err(format!("image not found on disk: {path_str}"));
    }

    let result = match pipeline.process_file(file_id, path, None) {
        Ok(res) => res,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "vision pipeline processing failed");
            let writer = db.writer();
            let _ = writer.execute(
                "UPDATE files SET status = 'error', error = ?1 WHERE id = ?2",
                rusqlite::params![err, file_id],
            );
            return Err(err);
        }
    };

    let mut writer = db.writer();
    pipeline
        .persist_results(&mut writer, &result)
        .map_err(|e| format!("failed to persist vision results: {e}"))?;

    let now = chrono::Utc::now().to_rfc3339();
    let _ = writer.execute(
        "UPDATE files SET status = 'active', error = NULL, last_indexed_at = ?1 WHERE id = ?2",
        rusqlite::params![now, file_id],
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::ClipConfig;
    use crate::ocr::OcrConfig;
    use image::{Rgb, RgbImage};
    use rusqlite::Connection;
    use smc_core::migrate::run_migrations;
    use tempfile::tempdir;

    #[test]
    fn test_vision_pipeline_end_to_end() {
        let tmp = tempdir().unwrap();
        let thumb_dir = tmp.path().join("thumbs");
        let models_dir = tmp.path().join("models");

        let ocr_engine = OcrEngine::new(OcrConfig::new(&models_dir));
        let clip_engine = ClipEngine::new(ClipConfig::new(&models_dir));
        let pipeline = VisionPipeline::new(thumb_dir, ocr_engine, clip_engine);

        // Create a test image
        let img_path = tmp.path().join("screenshot_2026-09-24.png");
        let mut img = RgbImage::new(1920, 1080);
        for pixel in img.pixels_mut() {
            *pixel = Rgb([50, 100, 150]);
        }
        img.save(&img_path).unwrap();

        let result = pipeline.process_file(42, &img_path, None).unwrap();
        assert_eq!(result.file_id, 42);
        assert_eq!(result.metadata.width, 1920);
        assert_eq!(result.metadata.height, 1080);
        assert!(result.metadata.is_screenshot);
        assert!(result.thumbnail_path.is_some());

        // Test DB persistence
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Insert a mock file row first to satisfy foreign key
        conn.execute(
            "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
             VALUES (42, 'C:/test.png', 'C:/', 'test.png', 'png', 100, '2026-09-24T00:00:00Z', '2026-09-24T00:00:00Z', 'image', 'active', '2026-09-24T00:00:00Z')",
            [],
        )
        .unwrap();

        let persist_res = pipeline.persist_results(&mut conn, &result);
        assert!(persist_res.is_ok());

        // Verify image_metadata row
        let is_screenshot: bool = conn
            .query_row(
                "SELECT is_screenshot FROM image_metadata WHERE file_id = 42",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(is_screenshot);

        // Verify image_tags row
        let tag_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM image_tags WHERE file_id = 42 AND tag = 'type:screenshot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tag_count, 1);
    }
}
