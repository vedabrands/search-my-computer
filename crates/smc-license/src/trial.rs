use chrono::Utc;
use hmac::{Hmac, Mac};
use rusqlite::Connection;
use sha2::Sha256;
use thiserror::Error;
use tracing::{info, warn};

use crate::license::LicenseStatus;

/// HMAC key derived from machine-local entropy. In production this would
/// incorporate a hardware fingerprint; for now we use a fixed app-secret
/// combined with the trial_start value to detect tampering.
const TRIAL_HMAC_SECRET: &[u8] = b"smc-trial-integrity-v1-2026";

const TRIAL_DURATION_DAYS: i64 = 14;

const META_KEY_TRIAL_START: &str = "trial_start_epoch";
const META_KEY_TRIAL_HMAC: &str = "trial_start_hmac";
const META_KEY_LAST_SEEN: &str = "last_seen_epoch";

#[derive(Debug, Error)]
pub enum TrialError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("trial data tampered")]
    Tampered,
}

/// Manages the 14-day local trial with clock rollback detection
/// and HMAC integrity verification. All state lives in the SQLite `meta` table.
pub struct TrialManager;

impl TrialManager {
    /// Compute HMAC-SHA256 tag over the trial start epoch.
    fn compute_hmac(epoch_secs: i64) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(TRIAL_HMAC_SECRET).expect("HMAC accepts any key size");
        mac.update(epoch_secs.to_le_bytes().as_ref());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Read a meta value from the `meta` table.
    fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>, TrialError> {
        let mut stmt = conn.prepare_cached("SELECT value FROM meta WHERE key = ?1")?;
        let result = stmt.query_row([key], |row| row.get::<_, String>(0)).ok();
        Ok(result)
    }

    /// Write a meta value to the `meta` table (upsert).
    fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), TrialError> {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Initialize or evaluate the trial state. Returns the current `LicenseStatus`.
    ///
    /// Clock rollback detection:
    /// - If `now < trial_start` → tampered / clock rolled back → expire immediately.
    /// - If `now < last_seen` → clock rolled back → expire immediately.
    /// - HMAC mismatch on `trial_start` → tampered → expire immediately.
    pub fn evaluate(conn: &Connection) -> Result<LicenseStatus, TrialError> {
        let now = Utc::now().timestamp();

        let trial_start_str = Self::get_meta(conn, META_KEY_TRIAL_START)?;
        let trial_hmac_str = Self::get_meta(conn, META_KEY_TRIAL_HMAC)?;
        let last_seen_str = Self::get_meta(conn, META_KEY_LAST_SEEN)?;

        let trial_start = match (trial_start_str, trial_hmac_str) {
            (Some(start_str), Some(hmac_str)) => {
                let start_epoch: i64 = start_str.parse().unwrap_or(0);

                // Verify HMAC integrity
                let expected_hmac = Self::compute_hmac(start_epoch);
                if hmac_str != expected_hmac {
                    warn!("trial HMAC integrity check failed — trial data may be tampered");
                    return Ok(LicenseStatus::TrialExpired);
                }

                // Clock rollback: now < trial_start
                if now < start_epoch {
                    warn!(
                        now_epoch = now,
                        trial_start_epoch = start_epoch,
                        "clock rollback detected (now < trial_start) — expiring trial"
                    );
                    return Ok(LicenseStatus::TrialExpired);
                }

                start_epoch
            }
            _ => {
                // First run: initialize trial
                info!(
                    trial_days = TRIAL_DURATION_DAYS,
                    "starting new trial period"
                );
                let hmac = Self::compute_hmac(now);
                Self::set_meta(conn, META_KEY_TRIAL_START, &now.to_string())?;
                Self::set_meta(conn, META_KEY_TRIAL_HMAC, &hmac)?;
                Self::set_meta(conn, META_KEY_LAST_SEEN, &now.to_string())?;
                now
            }
        };

        // Clock rollback: now < last_seen
        if let Some(last_seen_str) = last_seen_str {
            let last_seen: i64 = last_seen_str.parse().unwrap_or(0);
            // Allow up to 60 seconds of clock drift (NTP corrections, suspend/resume)
            if now < last_seen - 60 {
                warn!(
                    now_epoch = now,
                    last_seen_epoch = last_seen,
                    "clock rollback detected (now < last_seen) — expiring trial"
                );
                return Ok(LicenseStatus::TrialExpired);
            }
        }

        // Update last_seen watermark
        Self::set_meta(conn, META_KEY_LAST_SEEN, &now.to_string())?;

        // Calculate remaining days
        let elapsed_secs = now - trial_start;
        let elapsed_days = elapsed_secs / 86_400;
        let remaining = TRIAL_DURATION_DAYS - elapsed_days;

        if remaining <= 0 {
            info!(elapsed_days = elapsed_days, "trial period has expired");
            Ok(LicenseStatus::TrialExpired)
        } else {
            Ok(LicenseStatus::Trial {
                days_remaining: remaining as u32,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_fresh_trial_initializes() {
        let conn = setup_db();
        let status = TrialManager::evaluate(&conn).unwrap();
        match status {
            LicenseStatus::Trial { days_remaining } => {
                assert!((13..=14).contains(&days_remaining));
            }
            other => panic!("expected Trial, got {:?}", other),
        }
    }

    #[test]
    fn test_trial_hmac_integrity() {
        let conn = setup_db();
        // Initialize
        let _ = TrialManager::evaluate(&conn).unwrap();

        // Tamper with the start epoch
        conn.execute(
            "UPDATE meta SET value = '1000000000' WHERE key = 'trial_start_epoch'",
            [],
        )
        .unwrap();

        // HMAC mismatch should expire
        let status = TrialManager::evaluate(&conn).unwrap();
        assert!(status.is_expired_trial());
    }

    #[test]
    fn test_trial_expired_after_duration() {
        let conn = setup_db();
        // Set trial start to 15 days ago
        let past = Utc::now().timestamp() - (15 * 86_400);
        let hmac = TrialManager::compute_hmac(past);
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('trial_start_epoch', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&past.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('trial_start_hmac', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&hmac],
        )
        .unwrap();

        let status = TrialManager::evaluate(&conn).unwrap();
        assert!(status.is_expired_trial());
    }

    #[test]
    fn test_trial_clock_rollback_last_seen() {
        let conn = setup_db();
        // Initialize trial
        let _ = TrialManager::evaluate(&conn).unwrap();

        // Set last_seen far into the future to simulate clock rollback
        let future = Utc::now().timestamp() + 86_400 * 30; // 30 days ahead
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'last_seen_epoch'",
            [&future.to_string()],
        )
        .unwrap();

        let status = TrialManager::evaluate(&conn).unwrap();
        assert!(status.is_expired_trial());
    }

    #[test]
    fn test_hmac_deterministic() {
        let epoch = 1_727_000_000_i64;
        let h1 = TrialManager::compute_hmac(epoch);
        let h2 = TrialManager::compute_hmac(epoch);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_mid_trial() {
        let conn = setup_db();
        // Set trial start to 7 days ago
        let past = Utc::now().timestamp() - (7 * 86_400);
        let hmac = TrialManager::compute_hmac(past);
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('trial_start_epoch', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&past.to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('trial_start_hmac', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&hmac],
        )
        .unwrap();

        let status = TrialManager::evaluate(&conn).unwrap();
        match status {
            LicenseStatus::Trial { days_remaining } => {
                assert!((6..=7).contains(&days_remaining));
            }
            other => panic!("expected Trial, got {:?}", other),
        }
    }
}
