use crate::config::AppConfig;
use crate::db::Database;
use crate::error::{CoreError, CoreResult};
use crate::scanner::{ScanSnapshot, Scanner};
use crate::schema::{file_kind, file_status, job_kind, job_state};
use chrono::Utc;
use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use rusqlite::params;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

pub const DEFAULT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);
pub const MAX_BATCH_SIZE: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    CreatedOrModified,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct PendingChange {
    pub kind: FileChangeKind,
    pub timestamp: Instant,
}

pub struct FileWatcher {
    db: Database,
    config: Arc<Mutex<AppConfig>>,
    exclusions: Arc<Mutex<GlobSet>>,
    event_buffer: Arc<Mutex<HashMap<PathBuf, PendingChange>>>,
    is_running: Arc<AtomicBool>,
    watcher: Option<RecommendedWatcher>,
    debounce_window: Duration,
    flush_thread: Option<JoinHandle<()>>,
}

fn build_glob_set(patterns: &[String]) -> CoreResult<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder
        .build()
        .map_err(|e| CoreError::Scanner(format!("failed to compile glob set: {e}")))
}

impl FileWatcher {
    pub fn new(db: Database, config: Arc<Mutex<AppConfig>>) -> CoreResult<Self> {
        let initial_exclusions = {
            let cfg = config.lock();
            build_glob_set(&cfg.exclusions)?
        };

        Ok(Self {
            db,
            config,
            exclusions: Arc::new(Mutex::new(initial_exclusions)),
            event_buffer: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(AtomicBool::new(false)),
            watcher: None,
            debounce_window: DEFAULT_DEBOUNCE_WINDOW,
            flush_thread: None,
        })
    }

    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce_window = debounce;
        self
    }

    /// Refresh exclusion patterns from the current config.
    pub fn update_exclusions(&self) -> CoreResult<()> {
        let patterns = {
            let cfg = self.config.lock();
            cfg.exclusions.clone()
        };
        let new_set = build_glob_set(&patterns)?;
        *self.exclusions.lock() = new_set;
        Ok(())
    }

    /// Fast-path exclusion check before buffering or database operations.
    pub fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        let guard = self.exclusions.lock();
        guard.is_match(path_str.as_ref())
    }

    /// Records a filesystem change with debouncing and coalescing.
    pub fn record_change(&self, path: PathBuf, kind: FileChangeKind) {
        if self.is_excluded(&path) {
            debug!(path = %path.display(), "skipping excluded path event");
            return;
        }

        let mut buffer = self.event_buffer.lock();
        match kind {
            FileChangeKind::CreatedOrModified => {
                buffer.insert(
                    path,
                    PendingChange {
                        kind: FileChangeKind::CreatedOrModified,
                        timestamp: Instant::now(),
                    },
                );
            }
            FileChangeKind::Deleted => {
                if let Some(existing) = buffer.get(&path)
                    && existing.kind == FileChangeKind::CreatedOrModified
                    && existing.timestamp.elapsed() < self.debounce_window
                {
                    // Transient churn: created and deleted within debounce window -> cancel both
                    debug!(path = %path.display(), "cancelling transient churn event");
                    buffer.remove(&path);
                    return;
                }
                buffer.insert(
                    path,
                    PendingChange {
                        kind: FileChangeKind::Deleted,
                        timestamp: Instant::now(),
                    },
                );
            }
        }
    }

    /// Ingest a raw notify event.
    pub fn handle_notify_event(&self, event: notify::Result<Event>) {
        match event {
            Ok(ev) => {
                for path in ev.paths {
                    match ev.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            self.record_change(path, FileChangeKind::CreatedOrModified);
                        }
                        EventKind::Remove(_) => {
                            self.record_change(path, FileChangeKind::Deleted);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "watcher received error, will trigger full reconcile");
                let folders = {
                    let cfg = self.config.lock();
                    cfg.indexed_folders.clone()
                };
                let _ = self.reconcile_startup(&folders);
            }
        }
    }

    /// Run a fast startup reconciliation scan across all indexed folders.
    pub fn reconcile_startup(&self, folders: &[String]) -> CoreResult<ScanSnapshot> {
        info!("running startup reconciliation scan across watched folders");
        let cfg = self.config.lock().clone();
        let scanner = Scanner::new(self.db.clone(), &cfg)?;

        let mut total_snapshot = ScanSnapshot {
            files_seen: 0,
            files_indexed: 0,
            files_skipped: 0,
            files_deleted: 0,
            errors: 0,
        };

        for folder in folders {
            let path = Path::new(folder);
            if path.exists()
                && let Ok(snap) = scanner.scan_folder(path)
            {
                total_snapshot.files_seen += snap.files_seen;
                total_snapshot.files_indexed += snap.files_indexed;
                total_snapshot.files_skipped += snap.files_skipped;
                total_snapshot.files_deleted += snap.files_deleted;
                total_snapshot.errors += snap.errors;
            }
        }

        info!(
            files_seen = total_snapshot.files_seen,
            files_indexed = total_snapshot.files_indexed,
            files_deleted = total_snapshot.files_deleted,
            "startup reconciliation scan complete"
        );

        Ok(total_snapshot)
    }

    /// Flushes ready debounced changes to SQLite in a single transaction.
    pub fn flush_ready_changes(&self) -> CoreResult<usize> {
        let ready_items: Vec<(PathBuf, FileChangeKind)> = {
            let mut buffer = self.event_buffer.lock();
            let now = Instant::now();
            let mut ready = Vec::new();

            buffer.retain(|path, change| {
                if now.duration_since(change.timestamp) >= self.debounce_window
                    && ready.len() < MAX_BATCH_SIZE
                {
                    ready.push((path.clone(), change.kind));
                    false
                } else {
                    true
                }
            });

            ready
        };

        if ready_items.is_empty() {
            return Ok(0);
        }

        let max_size = {
            let cfg = self.config.lock();
            cfg.max_file_size
        };

        let mut conn = self.db.writer();
        let tx = conn.transaction()?;
        let now_iso = Utc::now().to_rfc3339();

        let mut processed = 0;

        {
            let mut upsert_file_stmt = tx.prepare_cached(
                "INSERT INTO files (path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(path) DO UPDATE SET
                    parent_dir = ?2, name = ?3, ext = ?4, size = ?5, mtime = ?6,
                    kind = ?8, status = ?9, last_indexed_at = ?10, error = NULL
                 RETURNING id",
            )?;

            let mut enqueue_job_stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                 SELECT ?1, ?2, 10, ?3, ?4
                 WHERE NOT EXISTS (
                     SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                 )",
            )?;

            let mut mark_deleted_stmt = tx.prepare_cached(
                "UPDATE files SET status = ?1, last_indexed_at = ?2 WHERE path = ?3 AND status != ?1",
            )?;

            for (path, kind) in ready_items {
                let normalized_path = path.to_string_lossy().replace('\\', "/");

                match kind {
                    FileChangeKind::CreatedOrModified => {
                        if !path.exists() || !path.is_file() {
                            // File was deleted before flush
                            mark_deleted_stmt.execute(params![
                                file_status::DELETED,
                                now_iso,
                                normalized_path
                            ])?;
                            processed += 1;
                            continue;
                        }

                        let metadata = match std::fs::metadata(&path) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };

                        if metadata.len() > max_size {
                            debug!(path = %path.display(), "file exceeds max size, skipping");
                            continue;
                        }

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

                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let ext = path
                            .extension()
                            .map(|e| e.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let parent_dir = path
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let file_kind_str = file_kind::classify(&ext);

                        let file_id: i64 = upsert_file_stmt.query_row(
                            params![
                                normalized_path,
                                parent_dir,
                                name,
                                ext,
                                size,
                                mtime,
                                ctime,
                                file_kind_str,
                                file_status::ACTIVE,
                                now_iso
                            ],
                            |row| row.get(0),
                        )?;

                        enqueue_job_stmt.execute(params![
                            job_kind::INDEX_FILENAME,
                            file_id,
                            job_state::PENDING,
                            now_iso
                        ])?;

                        processed += 1;
                    }
                    FileChangeKind::Deleted => {
                        let rows_affected = mark_deleted_stmt.execute(params![
                            file_status::DELETED,
                            now_iso,
                            normalized_path
                        ])?;
                        if rows_affected > 0 {
                            processed += 1;
                        }
                    }
                }
            }
        }

        tx.commit()?;
        Ok(processed)
    }

    /// Starts the background debounce flush thread and watches configured folders.
    pub fn start(&mut self, folders: &[String]) -> CoreResult<()> {
        if self.is_running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.is_running.store(true, Ordering::Relaxed);

        let buffer_arc = Arc::clone(&self.event_buffer);
        let exclusions_arc = Arc::clone(&self.exclusions);
        let db_clone = self.db.clone();
        let config_clone = Arc::clone(&self.config);
        let is_running_clone = Arc::clone(&self.is_running);
        let debounce = self.debounce_window;

        // Spawn background watcher flush loop
        self.flush_thread = Some(thread::spawn(move || {
            info!("started file watcher debounce flush thread");
            let watcher_helper = FileWatcher {
                db: db_clone,
                config: config_clone,
                exclusions: exclusions_arc,
                event_buffer: buffer_arc,
                is_running: is_running_clone.clone(),
                watcher: None,
                debounce_window: debounce,
                flush_thread: None,
            };

            while is_running_clone.load(Ordering::Relaxed) {
                if let Err(e) = watcher_helper.flush_ready_changes() {
                    error!(error = %e, "error flushing ready watcher changes");
                }
                thread::sleep(Duration::from_millis(100));
            }
            info!("file watcher debounce flush thread stopped");
        }));

        // Set up notify watcher
        let buffer_for_notify = Arc::clone(&self.event_buffer);
        let exclusions_for_notify = Arc::clone(&self.exclusions);
        let debounce_win = self.debounce_window;

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| match res {
                Ok(ev) => {
                    let exclusions = exclusions_for_notify.lock();
                    let mut buffer = buffer_for_notify.lock();
                    for path in ev.paths {
                        let path_str = path.to_string_lossy();
                        if exclusions.is_match(path_str.as_ref()) {
                            continue;
                        }

                        match ev.kind {
                            EventKind::Create(_) | EventKind::Modify(_) => {
                                buffer.insert(
                                    path,
                                    PendingChange {
                                        kind: FileChangeKind::CreatedOrModified,
                                        timestamp: Instant::now(),
                                    },
                                );
                            }
                            EventKind::Remove(_) => {
                                if let Some(existing) = buffer.get(&path)
                                    && existing.kind == FileChangeKind::CreatedOrModified
                                    && existing.timestamp.elapsed() < debounce_win
                                {
                                    buffer.remove(&path);
                                    continue;
                                }
                                buffer.insert(
                                    path,
                                    PendingChange {
                                        kind: FileChangeKind::Deleted,
                                        timestamp: Instant::now(),
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "notify watcher error");
                }
            },
            Config::default(),
        )
        .map_err(|e| CoreError::Scanner(format!("failed to initialize notify watcher: {e}")))?;

        for folder in folders {
            let path = Path::new(folder);
            if path.exists() && path.is_dir() {
                if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                    warn!(path = %path.display(), error = %e, "failed to watch directory");
                } else {
                    info!(path = %path.display(), "watching directory recursively");
                }
            }
        }

        self.watcher = Some(watcher);
        Ok(())
    }

    /// Stops the file watcher and background flush thread.
    pub fn stop(&mut self) {
        if !self.is_running.load(Ordering::Relaxed) {
            return;
        }

        self.is_running.store(false, Ordering::Relaxed);
        self.watcher.take();

        if let Some(handle) = self.flush_thread.take() {
            let _ = handle.join();
        }

        info!("file watcher stopped");
    }

    pub fn pending_event_count(&self) -> usize {
        self.event_buffer.lock().len()
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_watcher_fast_path_exclusion() {
        let db = Database::open_in_memory().unwrap();
        let mut config = AppConfig::default();
        config.exclusions.push("**/node_modules/**".to_string());
        config.exclusions.push("**/.git/**".to_string());
        let config_arc = Arc::new(Mutex::new(config));

        let watcher = FileWatcher::new(db, config_arc).unwrap();

        assert!(watcher.is_excluded(Path::new("C:/project/node_modules/package/index.js")));
        assert!(watcher.is_excluded(Path::new("C:/project/.git/objects/12/3456")));
        assert!(!watcher.is_excluded(Path::new("C:/project/src/index.rs")));
    }

    #[test]
    fn test_transient_churn_cancellation() {
        let db = Database::open_in_memory().unwrap();
        let config = AppConfig {
            exclusions: vec!["**/node_modules/**".into()],
            ..Default::default()
        };
        let config_arc = Arc::new(Mutex::new(config));
        let watcher = FileWatcher::new(db, config_arc)
            .unwrap()
            .with_debounce(Duration::from_millis(500));

        let test_path = PathBuf::from("C:/projects/docs/draft.md");

        // 1. Create file
        watcher.record_change(test_path.clone(), FileChangeKind::CreatedOrModified);
        assert_eq!(watcher.pending_event_count(), 1);

        // 2. Immediate Delete within debounce window -> cancels
        watcher.record_change(test_path.clone(), FileChangeKind::Deleted);
        assert_eq!(watcher.pending_event_count(), 0);
    }

    #[test]
    fn test_flush_ready_changes_to_db() {
        let db = Database::open_in_memory().unwrap();
        let config = AppConfig {
            exclusions: vec!["**/node_modules/**".into()],
            ..Default::default()
        };
        let config_arc = Arc::new(Mutex::new(config));
        let watcher = FileWatcher::new(db.clone(), config_arc)
            .unwrap()
            .with_debounce(Duration::from_millis(10));

        let dir = tempdir().unwrap();
        let test_file = dir.path().join("document.txt");
        fs::write(&test_file, "Hello from live watcher test").unwrap();

        watcher.record_change(test_file.clone(), FileChangeKind::CreatedOrModified);
        assert_eq!(watcher.pending_event_count(), 1);

        // Before debounce expiration, flush yields 0
        let flushed = watcher.flush_ready_changes().unwrap();
        assert_eq!(flushed, 0);

        // Wait for debounce window
        thread::sleep(Duration::from_millis(20));

        let flushed = watcher.flush_ready_changes().unwrap();
        assert_eq!(flushed, 1);
        assert_eq!(watcher.pending_event_count(), 0);

        // Verify file and job are in SQLite
        let reader = db.reader().unwrap();
        let count: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM files WHERE status = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let job_count: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE kind = 'index_filename'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(job_count, 1);
    }
}
