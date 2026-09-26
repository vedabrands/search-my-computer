use crate::config::AppConfig;
use crate::db::Database;
use crate::error::CoreResult;
use crate::schema::{file_kind, file_status, job_kind, job_state};
use chrono::Utc;
use globset::{Glob, GlobSet, GlobSetBuilder};
use parking_lot::Mutex;
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Progress counters shared with the UI.
#[derive(Debug, Default)]
pub struct ScanProgress {
    pub files_seen: AtomicU64,
    pub files_indexed: AtomicU64,
    pub files_skipped: AtomicU64,
    pub files_deleted: AtomicU64,
    pub errors: AtomicU64,
}

impl ScanProgress {
    pub fn snapshot(&self) -> ScanSnapshot {
        ScanSnapshot {
            files_seen: self.files_seen.load(Ordering::Relaxed),
            files_indexed: self.files_indexed.load(Ordering::Relaxed),
            files_skipped: self.files_skipped.load(Ordering::Relaxed),
            files_deleted: self.files_deleted.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScanSnapshot {
    pub files_seen: u64,
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub files_deleted: u64,
    pub errors: u64,
}

struct ScannedEntry {
    normalized_path: String,
    size: i64,
    mtime: String,
    ctime: String,
}

pub struct Scanner {
    db: Database,
    exclusions: GlobSet,
    max_file_size: u64,
    progress: Arc<ScanProgress>,
    cancel: Arc<AtomicBool>,
}

impl Scanner {
    pub fn new(db: Database, config: &AppConfig) -> CoreResult<Self> {
        let exclusions = build_glob_set(&config.exclusions)?;

        Ok(Self {
            db,
            exclusions,
            max_file_size: config.max_file_size,
            progress: Arc::new(ScanProgress::default()),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Get a reference to the progress counters.
    pub fn progress(&self) -> Arc<ScanProgress> {
        Arc::clone(&self.progress)
    }

    /// Request cancellation of an in-progress scan.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Scan a single directory tree. This is the main scan entry point.
    /// Runs in the calling thread (caller should spawn a thread for async usage).
    pub fn scan_folder(&self, root: &Path) -> CoreResult<ScanSnapshot> {
        info!(root = %root.display(), "starting scan");
        self.cancel.store(false, Ordering::Relaxed);

        // Collect the set of all canonical paths we see this scan.
        // Used for symlink loop detection on Windows (no inode available).
        let seen_canonical = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));

        // Collect all paths seen this scan for delete detection.
        let paths_this_scan = Arc::new(Mutex::new(HashSet::<String>::new()));

        let walker = WalkDir::new(root)
            .follow_links(true)
            .same_file_system(false);

        let mut batch: Vec<ScannedEntry> = Vec::with_capacity(1000);

        for entry in walker {
            if self.cancel.load(Ordering::Relaxed) {
                info!("scan cancelled");
                break;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "directory walk error (permission denied or broken link)");
                    self.progress.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };

            let path = entry.path();

            // Symlink loop detection: try to canonicalize and check for dupes.
            if entry.path_is_symlink() {
                match path.canonicalize() {
                    Ok(canonical) => {
                        let mut seen = seen_canonical.lock();
                        if !seen.insert(canonical) {
                            debug!(path = %path.display(), "skipping symlink loop");
                            continue;
                        }
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "cannot resolve symlink");
                        self.progress.errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                }
            }

            // Skip directories that match exclusion patterns.
            let path_str = path.to_string_lossy();
            if self.exclusions.is_match(path_str.as_ref()) {
                debug!(path = %path.display(), "excluded by glob pattern");
                continue;
            }

            // Only index files, not directories.
            if !entry.file_type().is_file() {
                continue;
            }

            self.progress.files_seen.fetch_add(1, Ordering::Relaxed);

            // Size check.
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "cannot read metadata");
                    self.progress.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };

            if metadata.len() > self.max_file_size {
                debug!(path = %path.display(), size = metadata.len(), "file too large, skipping");
                self.progress.files_skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let path_string = path_str.to_string();
            // Forward slashes for consistency in the DB.
            let normalized_path = path_string.replace('\\', "/");
            paths_this_scan.lock().insert(normalized_path.clone());

            let size = metadata.len() as i64;
            let mtime = metadata
                .modified()
                .map(|t| {
                    let dt: chrono::DateTime<Utc> = t.into();
                    dt.to_rfc3339()
                })
                .unwrap_or_default();
            let ctime = metadata
                .created()
                .map(|t| {
                    let dt: chrono::DateTime<Utc> = t.into();
                    dt.to_rfc3339()
                })
                .unwrap_or_default();

            batch.push(ScannedEntry {
                normalized_path,
                size,
                mtime,
                ctime,
            });

            if batch.len() >= 1000
                && let Err(e) = self.process_batch(&mut batch)
            {
                warn!(error = %e, "batch insert error during scan");
                self.progress.errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Process remaining items.
        if !batch.is_empty()
            && let Err(e) = self.process_batch(&mut batch)
        {
            warn!(error = %e, "final batch insert error during scan");
            self.progress.errors.fetch_add(1, Ordering::Relaxed);
        }

        // Mark files that were in the DB but not seen this scan as deleted.
        if !self.cancel.load(Ordering::Relaxed) {
            let root_str = root.to_string_lossy().replace('\\', "/");
            self.mark_deleted_files(&root_str, &paths_this_scan.lock())?;

            // Index project entities under this root.
            if let Err(e) = crate::project_detector::index_projects_in_root(
                root,
                Some(&self.exclusions),
                &self.db,
            ) {
                warn!(error = %e, root = ?root, "error indexing projects during scan");
            }
        }

        let snapshot = self.progress.snapshot();
        info!(
            files_seen = snapshot.files_seen,
            files_indexed = snapshot.files_indexed,
            files_skipped = snapshot.files_skipped,
            files_deleted = snapshot.files_deleted,
            errors = snapshot.errors,
            "scan complete"
        );

        Ok(snapshot)
    }

    /// Process a batch of scanned files in a single transaction for high throughput.
    fn process_batch(&self, batch: &mut Vec<ScannedEntry>) -> CoreResult<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let mut conn = self.db.writer();
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();

        {
            let mut check_stmt = tx.prepare_cached(
                "SELECT id, size, mtime FROM files WHERE path = ?1 AND status = 'active'",
            )?;
            let mut upsert_stmt = tx.prepare_cached(
                "INSERT INTO files (path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(path) DO UPDATE SET
                    parent_dir = ?2, name = ?3, ext = ?4, size = ?5, mtime = ?6,
                    kind = ?8, status = ?9, last_indexed_at = ?10, error = NULL
                 RETURNING id",
            )?;
            let mut job_stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                 SELECT ?1, ?2, 10, ?3, ?4
                 WHERE NOT EXISTS (
                     SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                 )",
            )?;

            for item in batch.drain(..) {
                let existing: Option<(i64, i64, String)> = check_stmt
                    .query_row([&item.normalized_path], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })
                    .ok();

                if let Some((_, existing_size, existing_mtime)) = existing
                    && existing_size == item.size
                    && existing_mtime == item.mtime
                {
                    self.progress.files_skipped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                let path_obj = Path::new(&item.normalized_path);
                let name = path_obj
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ext = path_obj
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default();
                let parent_dir = path_obj
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let kind = file_kind::classify(&ext);

                let file_id: i64 = match upsert_stmt.query_row(
                    params![
                        item.normalized_path,
                        parent_dir,
                        name,
                        ext,
                        item.size,
                        item.mtime,
                        item.ctime,
                        kind,
                        file_status::ACTIVE,
                        now
                    ],
                    |row| row.get(0),
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        warn!(path = %item.normalized_path, error = %e, "failed to upsert file");
                        self.progress.errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };

                let _ = job_stmt.execute(params![
                    job_kind::INDEX_FILENAME,
                    file_id,
                    job_state::PENDING,
                    now
                ]);

                self.progress.files_indexed.fetch_add(1, Ordering::Relaxed);
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Mark files under `root` that weren't seen this scan as deleted.
    fn mark_deleted_files(&self, root: &str, seen_paths: &HashSet<String>) -> CoreResult<()> {
        let mut conn = self.db.writer();
        let tx = conn.transaction()?;

        // Use LIKE prefix to find all active files under the root.
        let prefix = if root.ends_with('/') {
            root.to_string()
        } else {
            format!("{root}/")
        };

        let db_files: Vec<(i64, String)> = {
            let mut stmt =
                tx.prepare("SELECT id, path FROM files WHERE path LIKE ?1 AND status = 'active'")?;
            stmt.query_map([format!("{prefix}%")], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect()
        };

        let now = Utc::now().to_rfc3339();
        let mut deleted_count = 0u64;

        {
            let mut name_stmt = tx.prepare_cached("SELECT name FROM files WHERE id = ?1")?;
            let mut move_stmt = tx.prepare_cached(
                "UPDATE files SET path = ?1, parent_dir = ?2, last_indexed_at = ?3 WHERE id = ?4",
            )?;
            let mut delete_stmt = tx.prepare_cached(
                "UPDATE files SET status = ?1, last_indexed_at = ?2 WHERE id = ?3",
            )?;

            for (id, path) in &db_files {
                if !seen_paths.contains(path) {
                    let name: String = name_stmt.query_row([id], |row| row.get(0))?;

                    let moved_to = seen_paths.iter().find(|p| {
                        let p_path = Path::new(p.as_str());
                        p_path
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy() == name)
                    });

                    if let Some(new_path) = moved_to {
                        debug!(from = %path, to = %new_path, "detected file move");
                        let _ = move_stmt.execute(params![
                            new_path,
                            Path::new(new_path.as_str())
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            now,
                            id
                        ]);
                    } else {
                        let _ = delete_stmt.execute(params![file_status::DELETED, now, id]);
                        deleted_count += 1;
                    }
                }
            }
        }

        tx.commit()?;

        self.progress
            .files_deleted
            .fetch_add(deleted_count, Ordering::Relaxed);

        Ok(())
    }
}

fn build_glob_set(patterns: &[String]) -> CoreResult<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(e) => {
                warn!(pattern, error = %e, "invalid exclusion glob, skipping");
            }
        }
    }
    builder
        .build()
        .map_err(|e| crate::error::CoreError::Scanner(format!("failed to compile glob set: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::db::Database;
    use std::fs;

    fn test_db() -> Database {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        // Keep tmp alive by leaking — tests are short-lived.
        let db = Database::open(&db_path).unwrap();
        std::mem::forget(tmp);
        db
    }

    #[test]
    fn test_scan_basic_directory() {
        let tmp = tempfile::tempdir().unwrap();

        // Create some test files.
        fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
        fs::write(tmp.path().join("readme.md"), "# README").unwrap();
        fs::create_dir_all(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("code.rs"), "fn main() {}").unwrap();

        let db = test_db();
        let config = AppConfig {
            exclusions: vec![],
            ..Default::default()
        };
        let scanner = Scanner::new(db.clone(), &config).unwrap();
        let snapshot = scanner.scan_folder(tmp.path()).unwrap();

        assert_eq!(snapshot.files_seen, 3);
        assert_eq!(snapshot.files_indexed, 3);
        assert_eq!(snapshot.errors, 0);

        // Verify files are in the DB.
        let reader = db.reader().unwrap();
        let count: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM files WHERE status = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_scan_exclusions() {
        let tmp = tempfile::tempdir().unwrap();

        fs::write(tmp.path().join("keep.txt"), "keep").unwrap();
        fs::create_dir_all(tmp.path().join("node_modules")).unwrap();
        fs::write(
            tmp.path().join("node_modules").join("lib.js"),
            "module.exports = {};",
        )
        .unwrap();

        let db = test_db();
        let config = AppConfig {
            exclusions: vec!["**/node_modules/**".into()],
            ..Default::default()
        };
        let scanner = Scanner::new(db.clone(), &config).unwrap();
        let snapshot = scanner.scan_folder(tmp.path()).unwrap();

        assert_eq!(snapshot.files_indexed, 1);
    }

    #[test]
    fn test_scan_incremental_skip_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("stable.txt"), "unchanged").unwrap();

        let db = test_db();
        let config = AppConfig {
            exclusions: vec![],
            ..Default::default()
        };

        // First scan.
        let scanner = Scanner::new(db.clone(), &config).unwrap();
        let snap1 = scanner.scan_folder(tmp.path()).unwrap();
        assert_eq!(snap1.files_indexed, 1);

        // Second scan — same file, should be skipped.
        let scanner2 = Scanner::new(db.clone(), &config).unwrap();
        let snap2 = scanner2.scan_folder(tmp.path()).unwrap();
        assert_eq!(snap2.files_skipped, 1);
        assert_eq!(snap2.files_indexed, 0);
    }

    #[test]
    fn test_scan_detects_deleted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("will_delete.txt");
        fs::write(&file_path, "temp").unwrap();

        let db = test_db();
        let config = AppConfig {
            exclusions: vec![],
            ..Default::default()
        };

        // First scan.
        let scanner = Scanner::new(db.clone(), &config).unwrap();
        scanner.scan_folder(tmp.path()).unwrap();

        // Delete the file.
        fs::remove_file(&file_path).unwrap();

        // Second scan — should mark as deleted.
        let scanner2 = Scanner::new(db.clone(), &config).unwrap();
        let snap2 = scanner2.scan_folder(tmp.path()).unwrap();
        assert_eq!(snap2.files_deleted, 1);

        // Verify in DB.
        let reader = db.reader().unwrap();
        let status: String = reader
            .query_row(
                "SELECT status FROM files WHERE name = 'will_delete.txt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "deleted");
    }

    #[test]
    fn test_scan_max_file_size() {
        let tmp = tempfile::tempdir().unwrap();
        let big_file = tmp.path().join("big.bin");
        // 10-byte "big" file with a 5-byte limit.
        fs::write(&big_file, "0123456789").unwrap();
        fs::write(tmp.path().join("small.txt"), "hi").unwrap();

        let db = test_db();
        let config = AppConfig {
            exclusions: vec![],
            max_file_size: 5,
            ..Default::default()
        };
        let scanner = Scanner::new(db, &config).unwrap();
        let snap = scanner.scan_folder(tmp.path()).unwrap();
        assert_eq!(snap.files_indexed, 1);
        assert_eq!(snap.files_skipped, 1);
    }
}
