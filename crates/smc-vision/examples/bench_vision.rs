use image::{DynamicImage, RgbImage};
use qrcode::QrCode;
use smc_vision::metadata::ImageMetadata;
use smc_vision::qr::{classify_qr_payload, decode_barcodes};
use smc_vision::thumbnail::ThumbnailManager;
use std::path::Path;
use std::time::Instant;
use tempfile::tempdir;

fn create_test_qr_image(payload: &str) -> DynamicImage {
    let code = QrCode::new(payload.as_bytes()).unwrap();
    let img_buf = code
        .render::<image::Luma<u8>>()
        .min_dimensions(300, 300)
        .build();
    let dyn_img = DynamicImage::ImageLuma8(img_buf);
    dyn_img.to_rgb8().into()
}

fn main() {
    println!("=== SearchMyComputer Chunk 7 Vision Benchmarks ===");

    let tmp = tempdir().unwrap();
    let cache_dir = tmp.path().join("thumbnails");
    let thumb_manager = ThumbnailManager::new(cache_dir);

    // 1. Metadata & Screenshot Heuristics Benchmark
    println!("\n--- 1. Metadata & Screenshot Heuristics Benchmark ---");
    let path_screenshot = Path::new("C:/Users/dev/Pictures/Screenshot_2026-09-24.png");
    let count = 50000;
    let start = Instant::now();
    for _ in 0..count {
        let is_shot = ImageMetadata::detect_screenshot(path_screenshot, 1920, 1080, None, None);
        assert!(is_shot);
    }
    let elapsed = start.elapsed();
    let meta_ips = (count as f64) / elapsed.as_secs_f64();
    println!(
        "Metadata screenshot heuristics: {} iterations in {:.2?} => {:.2} images/sec",
        count, elapsed, meta_ips
    );

    // 2. Thumbnail Generation & Caching Benchmark
    println!("\n--- 2. Thumbnail Generation & Caching Benchmark ---");
    let test_img_photo = DynamicImage::ImageRgb8(RgbImage::new(1920, 1080));
    let img_path = tmp.path().join("photo_fhd.jpg");
    test_img_photo.save(&img_path).unwrap();

    let thumb_count = 50;
    let start = Instant::now();
    for i in 0..thumb_count {
        let hash = format!("sample_hash_{}", i);
        let _ = thumb_manager.get_or_create_thumbnail(&img_path, Some(&hash), None);
    }
    let elapsed = start.elapsed();
    let thumb_ips = (thumb_count as f64) / elapsed.as_secs_f64();
    println!(
        "Thumbnail generation (1080p -> 256px JPEG): {} images in {:.2?} => {:.2} images/sec",
        thumb_count, elapsed, thumb_ips
    );

    // 3. QR Decoding & Tag Classification Benchmark
    println!("\n--- 3. QR Decoding & Tag Classification Benchmark ---");
    let qr_img = create_test_qr_image("upi://pay?pa=merchant@okaxis&pn=Store&am=250.00");
    let qr_img_path = tmp.path().join("test_qr.png");
    qr_img.save(&qr_img_path).unwrap();

    let qr_count = 30;
    let start = Instant::now();
    for _ in 0..qr_count {
        let barcodes = decode_barcodes(&qr_img_path).unwrap();
        assert!(!barcodes.is_empty());
        assert_eq!(barcodes[0].tag, "qr:payment");
    }
    let elapsed = start.elapsed();
    let qr_ips = (qr_count as f64) / elapsed.as_secs_f64();
    println!(
        "QR Code decoding & classification (rxing): {} images in {:.2?} => {:.2} images/sec",
        qr_count, elapsed, qr_ips
    );

    // 4. Pure Tag Classification
    let class_count = 100000;
    let start = Instant::now();
    for _ in 0..class_count {
        let tag = classify_qr_payload("upi://pay?pa=merchant@okaxis&pn=Store&am=250.00");
        assert_eq!(tag, "qr:payment");
    }
    let elapsed = start.elapsed();
    let class_ips = (class_count as f64) / elapsed.as_secs_f64();
    println!(
        "Tag classification & masking: {} iterations in {:.2?} => {:.2} payloads/sec",
        class_count, elapsed, class_ips
    );

    // 5. DB Footprint Sizing Projection
    println!("\n--- 5. Database Footprint Sizing ---");
    let bytes_per_meta_row = 120; // file_id, width, height, format, exif_date, camera_make, camera_model, is_screenshot, has_qr
    let bytes_per_tag_row = 140; // file_id, tag, masked_payload, created_at
    let bytes_per_clip_vec = 512 * 2 + 32; // 512 f16 + overhead = 1,056 bytes
    let total_per_1000_images_no_clip = (bytes_per_meta_row + bytes_per_tag_row * 2) * 1000;
    let total_per_1000_images_with_clip =
        total_per_1000_images_no_clip + (bytes_per_clip_vec * 1000);

    println!(
        "DB footprint per 1,000 indexed images (Metadata + Tags): {:.2} KB",
        total_per_1000_images_no_clip as f64 / 1024.0
    );
    println!(
        "DB footprint per 1,000 indexed images (with 512-dim CLIP vectors): {:.2} MB",
        total_per_1000_images_with_clip as f64 / (1024.0 * 1024.0)
    );
}
