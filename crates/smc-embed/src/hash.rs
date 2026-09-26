use sha2::{Digest, Sha256};

/// Computes a hex-encoded SHA-256 hash of a chunk's text for change detection.
pub fn hash_chunk_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_chunk_text() {
        let h1 = hash_chunk_text("hello world");
        let h2 = hash_chunk_text("hello world");
        let h3 = hash_chunk_text("hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
    }
}
