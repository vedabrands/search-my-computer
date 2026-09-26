use crate::db::Database;
use crate::error::CoreResult;
use crate::governor::ResourceGovernor;
use crate::health::record_problem_file;
use crate::schema::{job_kind, job_state};
use chrono::Utc;
use rusqlite::params;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// A file-indexed job pulled from the queue.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub file_id: Option<i64>,
    pub priority: i64,
    pub attempts: i64,
}

/// Persistent, priority-based job queue backed by SQLite.
pub struct JobQueue {
    db: Database,
    running: Arc<AtomicBool>,
    worker_count: usize,
    governor: Option<Arc<ResourceGovernor>>,
}

impl JobQueue {
    pub fn new(db: Database, worker_count: usize) -> Self {
        Self {
            db,
            running: Arc::new(AtomicBool::new(false)),
            worker_count: worker_count.max(1),
            governor: None,
        }
    }

    pub fn with_governor(mut self, governor: Arc<ResourceGovernor>) -> Self {
        self.governor = Some(governor);
        self
    }

    pub fn governor(&self) -> Option<&Arc<ResourceGovernor>> {
        self.governor.as_ref()
    }

    /// Recover from a crash: reset all `running` jobs back to `pending`.
    pub fn recover(&self) -> CoreResult<usize> {
        let conn = self.db.writer();
        let reset = conn.execute(
            "UPDATE jobs SET state = ?1, attempts = attempts WHERE state = ?2",
            params![job_state::PENDING, job_state::RUNNING],
        )?;
        if reset > 0 {
            info!(count = reset, "recovered stale running jobs");
        }
        Ok(reset)
    }

    /// Enqueue a new job.
    pub fn enqueue(&self, kind: &str, file_id: Option<i64>, priority: i64) -> CoreResult<i64> {
        let conn = self.db.writer();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO jobs (kind, file_id, priority, state, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind, file_id, priority, job_state::PENDING, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Dequeue the highest-priority pending job atomically, respecting the Governor's allowed job kinds.
    pub fn dequeue(&self) -> CoreResult<Option<Job>> {
        let conn = self.db.writer();

        // If governor is present and indexing is user-paused or battery saver active, do not dequeue
        if let Some(ref gov) = self.governor
            && gov.allowed_worker_threads(self.worker_count) == 0
        {
            return Ok(None);
        }

        let mut stmt = conn.prepare_cached(
            "SELECT id, kind, file_id, priority, attempts FROM jobs
             WHERE state = ?1
             ORDER BY priority DESC, id ASC",
        )?;

        let mut rows = stmt.query([job_state::PENDING])?;
        let mut selected_job: Option<Job> = None;

        while let Some(row) = rows.next()? {
            let kind: String = row.get(1)?;
            if let Some(ref gov) = self.governor
                && !gov.is_job_allowed(&kind)
            {
                continue;
            }

            selected_job = Some(Job {
                id: row.get(0)?,
                kind,
                file_id: row.get(2)?,
                priority: row.get(3)?,
                attempts: row.get(4)?,
            });
            break;
        }

        drop(rows);
        drop(stmt);

        if let Some(ref job) = selected_job {
            conn.execute(
                "UPDATE jobs SET state = ?1, attempts = attempts + 1 WHERE id = ?2",
                params![job_state::RUNNING, job.id],
            )?;
        }

        Ok(selected_job)
    }

    /// Mark a job as completed.
    pub fn complete(&self, job_id: i64) -> CoreResult<()> {
        let conn = self.db.writer();
        conn.execute(
            "UPDATE jobs SET state = ?1 WHERE id = ?2",
            params![job_state::DONE, job_id],
        )?;
        Ok(())
    }

    /// Mark a job as failed with an error message. If max attempts (3) reached,
    /// marks as permanently failed and records to `problem_files`; otherwise resets to pending.
    pub fn fail(&self, job_id: i64, error: &str) -> CoreResult<()> {
        let conn = self.db.writer();

        let (attempts, file_id): (i64, Option<i64>) = conn.query_row(
            "SELECT attempts, file_id FROM jobs WHERE id = ?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        if attempts >= 3 {
            conn.execute(
                "UPDATE jobs SET state = ?1, error = ?2 WHERE id = ?3",
                params![job_state::FAILED, error, job_id],
            )?;

            // Record to problem_files if file_id exists
            if let Some(fid) = file_id {
                let file_path: Option<String> = conn
                    .query_row("SELECT path FROM files WHERE id = ?1", [fid], |row| {
                        row.get(0)
                    })
                    .ok();

                if let Some(path) = file_path {
                    let _ = record_problem_file(&self.db, Some(fid), &path, "job_failure", error);
                }
            }
        } else {
            conn.execute(
                "UPDATE jobs SET state = ?1, error = ?2 WHERE id = ?3",
                params![job_state::PENDING, error, job_id],
            )?;
        }

        Ok(())
    }

    /// Number of pending jobs.
    pub fn pending_count(&self) -> CoreResult<i64> {
        let reader = self.db.reader()?;
        let count: i64 = reader.query_row(
            "SELECT COUNT(*) FROM jobs WHERE state = ?1",
            [job_state::PENDING],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// Total counts by state (for UI status display).
    pub fn status_counts(&self) -> CoreResult<JobStatusCounts> {
        let reader = self.db.reader()?;
        let mut stmt = reader.prepare("SELECT state, COUNT(*) FROM jobs GROUP BY state")?;
        let mut counts = JobStatusCounts::default();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (state, count) = row?;
            match state.as_str() {
                "pending" => counts.pending = count,
                "running" => counts.running = count,
                "done" => counts.done = count,
                "failed" => counts.failed = count,
                _ => {}
            }
        }
        Ok(counts)
    }

    /// Start worker threads that process jobs until stopped.
    pub fn start_workers<F>(&self, process_fn: F)
    where
        F: Fn(&Job, &Database) -> Result<(), String> + Send + Sync + 'static,
    {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("job workers already running");
            return;
        }

        let process_fn = Arc::new(process_fn);

        for worker_id in 0..self.worker_count {
            let db = self.db.clone();
            let running = Arc::clone(&self.running);
            let process = Arc::clone(&process_fn);
            let governor = self.governor.clone();
            let total_workers = self.worker_count;

            thread::Builder::new()
                .name(format!("smc-worker-{worker_id}"))
                .spawn(move || {
                    info!(worker_id, "job worker started");

                    if let Some(ref gov) = governor {
                        gov.provider().set_thread_priority_below_normal();
                    }

                    let queue = JobQueue {
                        db: db.clone(),
                        running: running.clone(),
                        worker_count: total_workers,
                        governor: governor.clone(),
                    };

                    while running.load(Ordering::Relaxed) {
                        // Check if this worker_id is allowed under current governor state
                        if let Some(ref gov) = governor {
                            let allowed = gov.allowed_worker_threads(total_workers);
                            if worker_id >= allowed {
                                thread::sleep(Duration::from_millis(300));
                                continue;
                            }
                        }

                        match queue.dequeue() {
                            Ok(Some(job)) => {
                                debug!(worker_id, job_id = job.id, kind = %job.kind, "processing job");
                                match process(&job, &db) {
                                    Ok(()) => {
                                        if let Err(e) = queue.complete(job.id) {
                                            error!(job_id = job.id, error = %e, "failed to complete job");
                                        }
                                    }
                                    Err(e) => {
                                        warn!(job_id = job.id, error = %e, "job failed");
                                        if let Err(e2) = queue.fail(job.id, &e) {
                                            error!(job_id = job.id, error = %e2, "failed to mark job as failed");
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                // No jobs or work not currently allowed — sleep briefly.
                                thread::sleep(Duration::from_millis(200));
                            }
                            Err(e) => {
                                error!(worker_id, error = %e, "dequeue error");
                                thread::sleep(Duration::from_secs(1));
                            }
                        }
                    }

                    info!(worker_id, "job worker stopped");
                })
                .ok();
        }
    }

    /// Stop all worker threads (they'll finish current job then exit).
    pub fn stop_workers(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Whether workers are currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct JobStatusCounts {
    pub pending: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
}

/// Process a filename-indexing job.
pub fn process_job(job: &Job, db: &Database) -> Result<(), String> {
    match job.kind.as_str() {
        job_kind::INDEX_FILENAME => {
            if let Some(file_id) = job.file_id {
                let conn = db.writer();
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                     SELECT ?1, ?2, 5, ?3, ?4
                     WHERE NOT EXISTS (
                         SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                     )",
                    params![job_kind::EXTRACT, file_id, job_state::PENDING, now],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        job_kind::EXTRACT => {
            debug!(job_id = job.id, "extract job processing");
            Ok(())
        }
        other => Err(format!("unknown job kind: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::governor::MockSystemState;

    fn test_db() -> Database {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        std::mem::forget(tmp);
        db
    }

    #[test]
    fn test_enqueue_dequeue() {
        let db = test_db();
        let queue = JobQueue::new(db, 1);

        queue.enqueue("test_job", None, 5).unwrap();
        queue.enqueue("test_job", None, 10).unwrap();

        // Higher priority should come first.
        let job = queue.dequeue().unwrap().unwrap();
        assert_eq!(job.priority, 10);

        let job2 = queue.dequeue().unwrap().unwrap();
        assert_eq!(job2.priority, 5);

        // No more jobs.
        assert!(queue.dequeue().unwrap().is_none());
    }

    #[test]
    fn test_governor_integrated_dequeue() {
        let db = test_db();
        let mock = Arc::new(MockSystemState::new());
        let governor = Arc::new(ResourceGovernor::new(mock.clone()));
        let queue = JobQueue::new(db.clone(), 2).with_governor(governor.clone());

        // Enqueue EMBED and INDEX_FILENAME jobs
        queue.enqueue(job_kind::EMBED, None, 10).unwrap();
        queue.enqueue(job_kind::INDEX_FILENAME, None, 5).unwrap();

        // When on battery, EMBED is skipped and INDEX_FILENAME is dequeued
        mock.set_on_battery(true);
        let job = queue.dequeue().unwrap().unwrap();
        assert_eq!(job.kind, job_kind::INDEX_FILENAME);

        // When battery saver is active, nothing is dequeued
        mock.set_battery_saver(true);
        assert!(queue.dequeue().unwrap().is_none());

        // When AC is restored, EMBED is dequeued
        mock.set_battery_saver(false);
        mock.set_on_battery(false);
        let job = queue.dequeue().unwrap().unwrap();
        assert_eq!(job.kind, job_kind::EMBED);
    }

    #[test]
    fn test_complete_and_fail() {
        let db = test_db();
        let queue = JobQueue::new(db, 1);

        let id1 = queue.enqueue("test", None, 5).unwrap();
        let id2 = queue.enqueue("test", None, 5).unwrap();

        let job1 = queue.dequeue().unwrap().unwrap();
        queue.complete(job1.id).unwrap();

        let job2 = queue.dequeue().unwrap().unwrap();
        queue.fail(job2.id, "oops").unwrap();

        let counts = queue.status_counts().unwrap();
        assert_eq!(counts.done, 1);
        // Failed job goes back to pending (attempts < 3).
        assert_eq!(counts.pending, 1);

        // Test that the IDs match what we expected.
        assert_eq!(job1.id, id1);
        assert_eq!(job2.id, id2);
    }

    #[test]
    fn test_crash_recovery() {
        let db = test_db();
        let queue = JobQueue::new(db, 1);

        queue.enqueue("test", None, 5).unwrap();
        // Simulate crash: dequeue (sets to running) but don't complete.
        queue.dequeue().unwrap();

        let counts = queue.status_counts().unwrap();
        assert_eq!(counts.running, 1);

        // Recover: running → pending.
        let recovered = queue.recover().unwrap();
        assert_eq!(recovered, 1);

        let counts = queue.status_counts().unwrap();
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.running, 0);
    }

    #[test]
    fn test_max_attempts_permanent_failure() {
        let db = test_db();
        let queue = JobQueue::new(db, 1);

        queue.enqueue("test", None, 5).unwrap();

        // Fail 3 times — on the 3rd it should be permanently failed.
        for i in 0..3 {
            let job = queue.dequeue().unwrap().unwrap();
            queue.fail(job.id, &format!("error {i}")).unwrap();
        }

        let counts = queue.status_counts().unwrap();
        assert_eq!(counts.failed, 1);
        assert_eq!(counts.pending, 0);
    }
}
