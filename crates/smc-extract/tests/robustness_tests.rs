use smc_extract::extract_and_chunk;
use smc_extract::extractor::{ExtractionLimits, ExtractorRegistry};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_zero_byte_files_do_not_panic() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let extensions = [
        "txt", "md", "json", "csv", "docx", "pptx", "xlsx", "pdf", "rs", "py", "unknown",
    ];
    for ext in extensions {
        let file = NamedTempFile::with_suffix(format!(".{ext}")).unwrap();
        // File is 0 bytes
        let result = extract_and_chunk(&registry, file.path(), &limits);
        // It must never panic; it may succeed with empty doc or return an Err
        match result {
            Ok((doc, chunks)) => {
                // If it succeeds, chunks should be empty or safe
                assert!(chunks.is_empty() || doc.blocks.is_empty() || ext == "txt" || ext == "md");
            }
            Err(_) => {
                // An Err is completely acceptable for 0-byte docx/xlsx/pdf
            }
        }
    }
}

#[test]
fn test_corrupt_binary_garbage_fuzz() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let garbage_payloads: Vec<Vec<u8>> = vec![
        vec![0xFF; 512],
        vec![0x00; 1024],
        b"PK\x03\x04\x00\x00\x00\x00TruncatedZip".to_vec(),
        b"%PDF-1.4\n%corrupted pdf body without trailer\n%%EOF".to_vec(),
        b"<?xml version=\"1.0\"><unclosed_tag>No closing tag".to_vec(),
        (0..10_000).map(|i| (i % 256) as u8).collect(),
    ];

    let extensions = ["docx", "pptx", "xlsx", "pdf", "txt", "rs", "json"];

    for ext in extensions {
        for payload in &garbage_payloads {
            let mut file = NamedTempFile::with_suffix(format!(".{ext}")).unwrap();
            file.write_all(payload).unwrap();

            // Must never panic or hang
            let result = extract_and_chunk(&registry, file.path(), &limits);
            match result {
                Ok((doc, _chunks)) => {
                    // Plain text / code may decode with replacement characters
                    let _ = doc.full_text();
                }
                Err(_err) => {
                    // Zip or PDF parsers should return clean errors
                }
            }
        }
    }
}

#[test]
fn test_extraction_size_limit_enforced() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits {
        max_bytes: 1024, // 1 KB limit
        max_pages: 10,
        timeout_ms: 1000,
    };

    // Create a 5 KB file
    let mut file = NamedTempFile::with_suffix(".txt").unwrap();
    let large_data = vec![b'A'; 5120];
    file.write_all(&large_data).unwrap();

    let result = registry.extract_file(file.path(), &limits);
    assert!(
        result.is_err(),
        "Expected file exceeding max_bytes to return Err"
    );
    let err_msg = result.err().unwrap();
    assert!(err_msg.contains("exceeds limit"));
}

#[test]
fn test_malformed_xml_docx() {
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let mut temp = NamedTempFile::with_suffix(".docx").unwrap();
    {
        let mut zip = zip::ZipWriter::new(&mut temp);
        let options = zip::write::SimpleFileOptions::default();

        zip.start_file("word/document.xml", options).unwrap();
        // Malformed XML
        zip.write_all(b"<w:document><w:body><w:p><w:t>Unclosed paragraph")
            .unwrap();
        zip.finish().unwrap();
    }

    let result = extract_and_chunk(&registry, temp.path(), &limits);
    // Even with malformed XML, quick-xml will either stop or return error, no panic
    assert!(result.is_ok() || result.is_err());
}
