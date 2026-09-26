use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::debug;

/// Structured classification tag for decoded 1D/2D barcodes and QR codes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecodedBarcode {
    /// Canonical tag (e.g. `qr:payment`, `qr:url`, `qr:wifi`, `qr:contact`, `qr:text`).
    pub tag: String,
    /// Raw payload string stored in local DB.
    pub raw_payload: String,
    /// Masked payload for privacy-safe UI display.
    pub masked_payload: String,
}

impl DecodedBarcode {
    pub fn new(raw_payload: impl Into<String>) -> Self {
        let raw = raw_payload.into();
        let tag = classify_qr_payload(&raw);
        let masked_payload = mask_payload(&tag, &raw);
        Self {
            tag,
            raw_payload: raw,
            masked_payload,
        }
    }
}

/// Classify a decoded QR / barcode payload into a semantic tag.
pub fn classify_qr_payload(payload: &str) -> String {
    let lower = payload.trim().to_lowercase();

    // 1. Payment QR Codes
    // UPI (Unified Payments Interface)
    if lower.starts_with("upi://pay") || lower.starts_with("upi://") {
        return "qr:payment".to_string();
    }
    // EMVCo QR code specification (starts with format indicator 000201)
    if payload.trim().starts_with("000201") && payload.len() >= 20 {
        return "qr:payment".to_string();
    }
    // Cryptocurrency payment URIs
    if lower.starts_with("bitcoin:")
        || lower.starts_with("ethereum:")
        || lower.starts_with("solana:")
        || lower.starts_with("litecoin:")
        || lower.starts_with("bitcoincash:")
        || lower.starts_with("dogecoin:")
    {
        return "qr:payment".to_string();
    }
    // Web payment gateway URLs
    if lower.contains("paypal.me/")
        || lower.contains("venmo.com/")
        || lower.contains("cash.app/$")
        || lower.contains("revolut.me/")
        || lower.contains("pay.google.com")
        || lower.contains("paytm.me/")
    {
        return "qr:payment".to_string();
    }

    // 2. Wi-Fi Configuration
    if lower.starts_with("wifi:") || lower.starts_with("wifi;") {
        return "qr:wifi".to_string();
    }

    // 3. Contact cards (vCard / MeCard / BizCard)
    if lower.starts_with("begin:vcard")
        || lower.starts_with("mecard:")
        || lower.starts_with("bizcard:")
    {
        return "qr:contact".to_string();
    }

    // 4. Communication links (email, phone, sms)
    if lower.starts_with("mailto:") || lower.starts_with("matmsg:") {
        return "qr:email".to_string();
    }
    if lower.starts_with("tel:") {
        return "qr:tel".to_string();
    }
    if lower.starts_with("sms:") || lower.starts_with("smsto:") {
        return "qr:sms".to_string();
    }

    // 5. Standard Web URLs
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("ftp://")
    {
        return "qr:url".to_string();
    }

    // 6. Generic Text fallback
    "qr:text".to_string()
}

/// Mask sensitive account/credential info in QR payloads for safe UI preview.
pub fn mask_payload(tag: &str, raw: &str) -> String {
    match tag {
        "qr:payment" => {
            let lower = raw.to_lowercase();
            // UPI mask: upi://pay?pa=account@bank&pn=Name -> upi://pay?pa=acc***@bank&pn=Name
            if lower.starts_with("upi://") {
                if let Some(pa_idx) = lower.find("pa=") {
                    let after_pa = &raw[pa_idx + 3..];
                    let end_idx = after_pa.find('&').unwrap_or(after_pa.len());
                    let vpa = &after_pa[..end_idx];

                    if let Some(at_idx) = vpa.find('@') {
                        let handle = &vpa[..at_idx];
                        let bank = &vpa[at_idx..];
                        let masked_handle = if handle.len() > 3 {
                            format!("{}***{}", &handle[..2], &handle[handle.len() - 1..])
                        } else {
                            "***".to_string()
                        };
                        let masked_vpa = format!("{}{}", masked_handle, bank);
                        return format!(
                            "{}{}{}",
                            &raw[..pa_idx + 3],
                            masked_vpa,
                            &after_pa[end_idx..]
                        );
                    }
                }
                return "upi://pay?pa=***".to_string();
            }

            // Crypto URI mask: bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=...
            if let Some(colon_idx) = raw.find(':') {
                let prefix = &raw[..=colon_idx];
                let rest = &raw[colon_idx + 1..];
                let end_addr = rest.find('?').unwrap_or(rest.len());
                let addr = &rest[..end_addr];
                let query = &rest[end_addr..];

                if addr.len() > 8 {
                    let masked_addr = format!("{}...{}", &addr[..4], &addr[addr.len() - 4..]);
                    return format!("{}{}{}", prefix, masked_addr, query);
                }
            }

            // EMVCo payload mask
            if raw.starts_with("000201") && raw.len() > 12 {
                return format!("{}...[EMVCo Payment QR]", &raw[..12]);
            }

            // Generic payment link mask
            "*** [Payment QR Details Protected]".to_string()
        }
        "qr:wifi" => {
            // WIFI:S:MySSID;T:WPA;P:MyPassword;; -> WIFI:S:MySSID;T:WPA;P:***;;
            let mut result = raw.to_string();
            if let Some(p_start) = raw.find(";P:") {
                let after_p = &raw[p_start + 3..];
                if let Some(p_end) = after_p.find(';') {
                    result = format!("{};P:***{}", &raw[..p_start], &after_p[p_end..]);
                }
            } else if let Some(p_start) = raw.find("P:") {
                let after_p = &raw[p_start + 2..];
                if let Some(p_end) = after_p.find(';') {
                    result = format!("{}P:***{}", &raw[..p_start], &after_p[p_end..]);
                }
            }
            result
        }
        _ => raw.to_string(),
    }
}

/// Decode barcodes and QR codes from an image file using rxing.
pub fn decode_barcodes(path: &Path) -> Result<Vec<DecodedBarcode>, String> {
    let path_str = path.to_string_lossy().to_string();

    let mut results = Vec::new();

    // Try multi-barcode detection in file first
    if let Ok(barcode_results) = rxing::helpers::detect_multiple_in_file(&path_str) {
        for res in barcode_results {
            let text = res.getText();
            if !text.trim().is_empty() {
                debug!(format = ?res.getBarcodeFormat(), "barcode decoded successfully");
                results.push(DecodedBarcode::new(text));
            }
        }
    } else if let Ok(single_res) = rxing::helpers::detect_in_file(&path_str, None) {
        let text = single_res.getText();
        if !text.trim().is_empty() {
            results.push(DecodedBarcode::new(text));
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_qr_payloads() {
        assert_eq!(
            classify_qr_payload("upi://pay?pa=merchant@okicici&pn=Merchant&am=100"),
            "qr:payment"
        );
        assert_eq!(
            classify_qr_payload("00020101021126580014COM.PAYPAL..."),
            "qr:payment"
        );
        assert_eq!(
            classify_qr_payload("bitcoin:1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa?amount=0.05"),
            "qr:payment"
        );
        assert_eq!(
            classify_qr_payload("https://paypal.me/johndoe/25"),
            "qr:payment"
        );
        assert_eq!(
            classify_qr_payload("https://github.com/searchmycomputer"),
            "qr:url"
        );
        assert_eq!(
            classify_qr_payload("WIFI:S:Home_5G;T:WPA;P:SuperSecret123;;"),
            "qr:wifi"
        );
        assert_eq!(
            classify_qr_payload("BEGIN:VCARD\nVERSION:3.0\nN:Doe;John\nFN:John Doe\nEND:VCARD"),
            "qr:contact"
        );
        assert_eq!(classify_qr_payload("Just a plain note barcode"), "qr:text");
    }

    #[test]
    fn test_mask_sensitive_payloads() {
        let upi = "upi://pay?pa=johnsmith@okhdfcbank&pn=John Smith";
        let masked_upi = mask_payload("qr:payment", upi);
        assert!(!masked_upi.contains("johnsmith@"));
        assert!(masked_upi.contains("@okhdfcbank"));
        assert!(masked_upi.contains("jo***h@"));

        let wifi = "WIFI:S:Home_5G;T:WPA;P:SuperSecret123;;";
        let masked_wifi = mask_payload("qr:wifi", wifi);
        assert!(!masked_wifi.contains("SuperSecret123"));
        assert_eq!(masked_wifi, "WIFI:S:Home_5G;T:WPA;P:***;;");
    }

    #[test]
    fn test_qr_code_image_decoding() {
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let qr_path = tmp.path().join("payment_qr.png");

        let payload = "upi://pay?pa=store@okaxis&pn=Store&am=250";
        let code = qrcode::QrCode::new(payload.as_bytes()).unwrap();
        let img = code.render::<image::Luma<u8>>().build();
        img.save(&qr_path).unwrap();

        let decoded = decode_barcodes(&qr_path).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].tag, "qr:payment");
        assert_eq!(decoded[0].raw_payload, payload);
        assert!(decoded[0].masked_payload.contains("@okaxis"));
        assert!(!decoded[0].masked_payload.contains("store@"));
    }
}
