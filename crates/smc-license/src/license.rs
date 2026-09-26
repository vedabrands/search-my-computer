use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Embedded Ed25519 public key for license verification.
/// This is the ONLY key the shipping binary trusts.
/// Generated via `license-tool keygen` and pasted here as raw 32 bytes.
///
/// PLACEHOLDER: Replace with your real public key before release.
/// Run: `license-tool keygen --output keys/`
/// Then paste the hex bytes from `keys/public_key.hex` here.
pub const EMBEDDED_PUBLIC_KEY: [u8; 32] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Debug, Error)]
pub enum LicenseError {
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid signature encoding: {0}")]
    InvalidSignatureEncoding(String),

    #[error("invalid public key")]
    InvalidPublicKey,

    #[error("signature verification failed")]
    SignatureVerificationFailed,

    #[error("license is for product '{found}', expected '{expected}'")]
    ProductMismatch { found: String, expected: String },

    #[error("license is for major version {found}, app is version {expected}")]
    VersionMismatch { found: u32, expected: u32 },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// On-disk license file schema. The `signature` field covers the canonical
/// JSON serialization of all other fields (sorted keys, no whitespace).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseFile {
    pub license_id: String,
    pub email_hash: String,
    pub customer_name: String,
    pub product: String,
    pub major_version: u32,
    pub issued_at: String,
    pub max_devices: u32,
    pub signature: String,
}

/// The subset of fields that the signature covers.
/// Serialized with sorted keys and no extra whitespace.
#[derive(Serialize)]
struct LicensePayload {
    customer_name: String,
    email_hash: String,
    issued_at: String,
    license_id: String,
    major_version: u32,
    max_devices: u32,
    product: String,
}

impl LicenseFile {
    /// Reconstruct the canonical payload bytes that were signed.
    fn canonical_payload(&self) -> Vec<u8> {
        let payload = LicensePayload {
            customer_name: self.customer_name.clone(),
            email_hash: self.email_hash.clone(),
            issued_at: self.issued_at.clone(),
            license_id: self.license_id.clone(),
            major_version: self.major_version,
            max_devices: self.max_devices,
            product: self.product.clone(),
        };
        // serde_json with sorted struct fields (struct order = alphabetical above)
        serde_json::to_vec(&payload).expect("payload serialization is infallible")
    }
}

/// Current runtime license status shown to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status")]
pub enum LicenseStatus {
    #[serde(rename = "trial")]
    Trial { days_remaining: u32 },

    #[serde(rename = "trial_expired")]
    TrialExpired,

    #[serde(rename = "licensed")]
    Licensed {
        customer_name: String,
        license_id: String,
    },
}

impl LicenseStatus {
    pub fn is_expired_trial(&self) -> bool {
        matches!(self, LicenseStatus::TrialExpired)
    }

    pub fn is_licensed(&self) -> bool {
        matches!(self, LicenseStatus::Licensed { .. })
    }
}

/// Verify a license JSON string against the embedded public key.
/// Returns the parsed `LicenseFile` on success.
pub fn verify_license(
    license_json: &str,
    pub_key_bytes: &[u8; 32],
    expected_product: &str,
    expected_major_version: u32,
) -> Result<LicenseFile, LicenseError> {
    let license: LicenseFile = serde_json::from_str(license_json)?;

    // Product and version gate
    if license.product != expected_product {
        return Err(LicenseError::ProductMismatch {
            found: license.product.clone(),
            expected: expected_product.to_string(),
        });
    }
    if license.major_version != expected_major_version {
        return Err(LicenseError::VersionMismatch {
            found: license.major_version,
            expected: expected_major_version,
        });
    }

    // Decode the base64 signature
    let sig_bytes = decode_base64(&license.signature)
        .map_err(|e| LicenseError::InvalidSignatureEncoding(e.to_string()))?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|_| LicenseError::InvalidSignatureEncoding("not 64 bytes".into()))?;

    // Reconstruct canonical payload and verify
    let payload = license.canonical_payload();
    let verifying_key =
        VerifyingKey::from_bytes(pub_key_bytes).map_err(|_| LicenseError::InvalidPublicKey)?;

    verifying_key
        .verify(&payload, &signature)
        .map_err(|_| LicenseError::SignatureVerificationFailed)?;

    Ok(license)
}

/// Minimal base64 decoder (standard alphabet, optional padding).
/// Avoids adding the `base64` crate to the shipping binary.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in input.as_bytes() {
        let val = ALPHABET
            .iter()
            .position(|&c| c == b)
            .ok_or_else(|| format!("invalid base64 character: {}", b as char))?
            as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub const PRODUCT_NAME: &str = "SearchMyComputer";
pub const MAJOR_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode_roundtrip() {
        // "Hello" -> "SGVsbG8="
        let decoded = decode_base64("SGVsbG8=").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn test_base64_decode_no_padding() {
        let decoded = decode_base64("SGVsbG8").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn test_base64_decode_invalid_char() {
        assert!(decode_base64("SGVs!G8=").is_err());
    }

    #[test]
    fn test_license_canonical_payload_deterministic() {
        let lic = LicenseFile {
            license_id: "LIC-001".into(),
            email_hash: "abc123".into(),
            customer_name: "Alice".into(),
            product: "SearchMyComputer".into(),
            major_version: 1,
            issued_at: "2026-09-01T00:00:00Z".into(),
            max_devices: 3,
            signature: String::new(),
        };
        let p1 = lic.canonical_payload();
        let p2 = lic.canonical_payload();
        assert_eq!(p1, p2);

        // Verify sorted key order in the JSON
        let json_str = String::from_utf8(p1).unwrap();
        let keys: Vec<&str> = vec![
            "customer_name",
            "email_hash",
            "issued_at",
            "license_id",
            "major_version",
            "max_devices",
            "product",
        ];
        let mut last_pos = 0;
        for key in keys {
            let pos = json_str.find(&format!("\"{}\"", key)).unwrap();
            assert!(pos >= last_pos, "key '{}' out of alphabetical order", key);
            last_pos = pos;
        }
    }

    #[test]
    fn test_verify_wrong_product() {
        let json = serde_json::json!({
            "license_id": "LIC-001",
            "email_hash": "abc",
            "customer_name": "Bob",
            "product": "OtherApp",
            "major_version": 1,
            "issued_at": "2026-01-01T00:00:00Z",
            "max_devices": 1,
            "signature": "AAAA"
        });
        let result = verify_license(&json.to_string(), &[0u8; 32], "SearchMyComputer", 1);
        assert!(matches!(result, Err(LicenseError::ProductMismatch { .. })));
    }

    #[test]
    fn test_verify_wrong_version() {
        let json = serde_json::json!({
            "license_id": "LIC-001",
            "email_hash": "abc",
            "customer_name": "Bob",
            "product": "SearchMyComputer",
            "major_version": 2,
            "issued_at": "2026-01-01T00:00:00Z",
            "max_devices": 1,
            "signature": "AAAA"
        });
        let result = verify_license(&json.to_string(), &[0u8; 32], "SearchMyComputer", 1);
        assert!(matches!(result, Err(LicenseError::VersionMismatch { .. })));
    }

    #[test]
    fn test_license_status_serde() {
        let trial = LicenseStatus::Trial { days_remaining: 10 };
        let json = serde_json::to_string(&trial).unwrap();
        assert!(json.contains("\"status\":\"trial\""));
        assert!(json.contains("\"days_remaining\":10"));

        let expired = LicenseStatus::TrialExpired;
        assert!(expired.is_expired_trial());
        assert!(!expired.is_licensed());

        let licensed = LicenseStatus::Licensed {
            customer_name: "Alice".into(),
            license_id: "LIC-001".into(),
        };
        assert!(licensed.is_licensed());
        assert!(!licensed.is_expired_trial());
    }
}
