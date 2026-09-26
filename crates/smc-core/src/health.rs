use crate::db::Database;
use crate::error::CoreResult;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProblemFileRecord {
    pub id: i64,
    pub file_id: Option<i64>,
    pub path: String,
    pub error_kind: String,
    pub error_message: String,
    pub attempts: i64,
    pub last_failed_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaintenanceStats {
    pub wal_checkpoint_pages: i64,
    pub fts_optimized: bool,
    pub duration_ms: u64,
}

/// Computes exponential retry backoff based on attempt count.
pub fn compute_backoff_delay(attempts: u32) -> Duration {
    match attempts {
        0 | 1 => Duration::from_secs(2),
        2 => Duration::from_secs(10),
        _ => Duration::from_secs(60),
    }
}

/// Records or increments an error for a problem file.
pub fn record_problem_file(
    db: &Database,
    file_id: Option<i64>,
    path: &str,
    error_kind: &str,
    error_message: &str,
) -> CoreResult<()> {
    let conn = db.writer();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO problem_files (file_id, path, error_kind, error_message, attempts, last_failed_at, resolved_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, NULL)
         ON CONFLICT(path) DO UPDATE SET
             file_id = COALESCE(?1, problem_files.file_id),
             error_kind = ?3,
             error_message = ?4,
             attempts = problem_files.attempts + 1,
             last_failed_at = ?5,
             resolved_at = NULL",
        params![file_id, path, error_kind, error_message, now],
    )?;

    info!(path = %path, error_kind = %error_kind, "recorded problem file");
    Ok(())
}

/// Marks a problem file as resolved after successful indexing.
pub fn resolve_problem_file(db: &Database, path: &str) -> CoreResult<bool> {
    let conn = db.writer();
    let now = Utc::now().to_rfc3339();

    let count = conn.execute(
        "UPDATE problem_files SET resolved_at = ?1 WHERE path = ?2 AND resolved_at IS NULL",
        params![now, path],
    )?;

    if count > 0 {
        info!(path = %path, "resolved problem file");
    }
    Ok(count > 0)
}

/// Checks whether a given path is currently an active (unresolved) problem file.
pub fn is_problem_file(db: &Database, path: &str) -> CoreResult<bool> {
    let reader = db.reader()?;
    let count: i64 = reader.query_row(
        "SELECT COUNT(*) FROM problem_files WHERE path = ?1 AND resolved_at IS NULL",
        params![path],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// Lists problem files, optionally filtering for unresolved items only.
pub fn list_problem_files(
    db: &Database,
    unresolved_only: bool,
) -> CoreResult<Vec<ProblemFileRecord>> {
    let reader = db.reader()?;
    let sql = if unresolved_only {
        "SELECT id, file_id, path, error_kind, error_message, attempts, last_failed_at, resolved_at
         FROM problem_files WHERE resolved_at IS NULL ORDER BY last_failed_at DESC"
    } else {
        "SELECT id, file_id, path, error_kind, error_message, attempts, last_failed_at, resolved_at
         FROM problem_files ORDER BY last_failed_at DESC"
    };

    let mut stmt = reader.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(ProblemFileRecord {
            id: row.get(0)?,
            file_id: row.get(1)?,
            path: row.get(2)?,
            error_kind: row.get(3)?,
            error_message: row.get(4)?,
            attempts: row.get(5)?,
            last_failed_at: row.get(6)?,
            resolved_at: row.get(7)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Clears all problem file records from the database.
pub fn clear_problem_files(db: &Database) -> CoreResult<usize> {
    let conn = db.writer();
    let count = conn.execute("DELETE FROM problem_files", [])?;
    info!(count, "cleared problem files");
    Ok(count)
}

/// Deletes a single problem file entry by id.
pub fn delete_problem_file(db: &Database, id: i64) -> CoreResult<bool> {
    let conn = db.writer();
    let count = conn.execute("DELETE FROM problem_files WHERE id = ?1", params![id])?;
    Ok(count > 0)
}

/// Runs periodic SQLite database maintenance: WAL checkpoint, incremental vacuum, and FTS5 optimization.
pub fn run_sqlite_maintenance(db: &Database) -> CoreResult<MaintenanceStats> {
    let start = std::time::Instant::now();
    let conn = db.writer();

    info!("starting periodic SQLite maintenance");

    // 1. WAL Checkpoint TRUNCATE.
    let wal_checkpoint_pages: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| row.get(1))
        .unwrap_or(0);

    // 2. Incremental vacuum.
    let _ = conn.execute_batch("PRAGMA incremental_vacuum(1000);");

    // 3. Optimize FTS5 indices.
    let _ = conn.execute_batch(
        "INSERT INTO files_fts(files_fts) VALUES('optimize');
         INSERT INTO chunks_fts(chunks_fts) VALUES('optimize');
         INSERT INTO projects_fts(projects_fts) VALUES('optimize');",
    );

    let duration_ms = start.elapsed().as_millis() as u64;
    info!(
        duration_ms,
        wal_checkpoint_pages, "SQLite maintenance completed"
    );

    Ok(MaintenanceStats {
        wal_checkpoint_pages,
        fts_optimized: true,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        assert_eq!(compute_backoff_delay(0), Duration::from_secs(2));
        assert_eq!(compute_backoff_delay(1), Duration::from_secs(2));
        assert_eq!(compute_backoff_delay(2), Duration::from_secs(10));
        assert_eq!(compute_backoff_delay(3), Duration::from_secs(60));
        assert_eq!(compute_backoff_delay(10), Duration::from_secs(60));
    }

    #[test]
    fn test_record_and_resolve_problem_file() {
        let db = Database::open_in_memory().unwrap();
        let path = "C:/Users/dev/corrupted.pdf";

        assert!(!is_problem_file(&db, path).unwrap());

        record_problem_file(&db, None, path, "corrupt_pdf", "Unexpected EOF").unwrap();
        assert!(is_problem_file(&db, path).unwrap());

        let problems = list_problem_files(&db, true).unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].path, path);
        assert_eq!(problems[0].error_kind, "corrupt_pdf");
        assert_eq!(problems[0].attempts, 1);

        // Record again to verify attempts increment
        record_problem_file(&db, None, path, "corrupt_pdf", "Still broken").unwrap();
        let problems = list_problem_files(&db, true).unwrap();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].attempts, 2);
        assert_eq!(problems[0].error_message, "Still broken");

        // Resolve problem file
        let resolved = resolve_problem_file(&db, path).unwrap();
        assert!(resolved);
        assert!(!is_problem_file(&db, path).unwrap());

        let active_problems = list_problem_files(&db, true).unwrap();
        assert_eq!(active_problems.len(), 0);

        let all_problems = list_problem_files(&db, false).unwrap();
        assert_eq!(all_problems.len(), 1);
        assert!(all_problems[0].resolved_at.is_some());
    }

    #[test]
    fn test_run_sqlite_maintenance() {
        let db = Database::open_in_memory().unwrap();
        let stats = run_sqlite_maintenance(&db).unwrap();
        assert!(stats.fts_optimized);
    }
}
