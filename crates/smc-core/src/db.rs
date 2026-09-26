use crate::error::{CoreError, CoreResult};
use crate::migrate;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;

/// Project entity record in the database.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ProjectRecord {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub project_type: String,
    pub manifest_path: Option<String>,
    pub readme_summary: Option<String>,
    pub last_detected_at: String,
}

/// Central database handle: one exclusive writer + a read pool.
#[derive(Clone)]
pub struct Database {
    writer: Arc<Mutex<Connection>>,
    reader_pool: Pool<SqliteConnectionManager>,
    path: PathBuf,
}

/// SQLite pragmas applied to every connection.
fn apply_pragmas(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -64000;
         PRAGMA journal_size_limit = 6291456;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

impl Database {
    /// Open or create the database at `path`. Runs migrations and sets up the
    /// connection pool.
    pub fn open(path: &Path) -> CoreResult<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| CoreError::Config("invalid database path".into()))?;

        // Writer connection.
        let writer = Connection::open(path_str)?;
        apply_pragmas(&writer)?;
        migrate::run_migrations(&writer)?;

        // Reader pool.
        let manager = SqliteConnectionManager::file(path_str);
        let reader_pool = Pool::builder()
            .max_size(4)
            .min_idle(Some(1))
            .connection_customizer(Box::new(PragmaCustomizer))
            .build(manager)?;

        info!(?path, "database opened");

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            reader_pool,
            path: path.to_path_buf(),
        })
    }

    /// Open an in-memory / temporary database (for tests).
    pub fn open_in_memory() -> CoreResult<Self> {
        let tmp = tempfile::NamedTempFile::new()?;
        let (_file, path) = tmp.keep().map_err(|e| CoreError::Io(e.error))?;

        let writer = Connection::open(&path)?;
        apply_pragmas(&writer)?;
        migrate::run_migrations(&writer)?;

        let manager = SqliteConnectionManager::file(&path);
        let reader_pool = Pool::builder()
            .max_size(2)
            .connection_customizer(Box::new(PragmaCustomizer))
            .build(manager)?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            reader_pool,
            path,
        })
    }

    /// Get exclusive write access. The returned guard holds the lock.
    pub fn writer(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.writer.lock()
    }

    /// Get a read-only connection from the pool.
    pub fn reader(&self) -> CoreResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        Ok(self.reader_pool.get()?)
    }

    /// Returns the database file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Upsert a project record into `projects`.
    pub fn upsert_project(&self, project: &ProjectRecord) -> CoreResult<i64> {
        let conn = self.writer();
        let id: i64 = conn.query_row(
            "INSERT INTO projects (path, name, project_type, manifest_path, readme_summary, last_detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                name = ?2,
                project_type = ?3,
                manifest_path = ?4,
                readme_summary = ?5,
                last_detected_at = ?6
             RETURNING id",
            rusqlite::params![
                project.path,
                project.name,
                project.project_type,
                project.manifest_path,
                project.readme_summary,
                project.last_detected_at,
            ],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// List all projects sorted by last_detected_at DESC.
    pub fn list_projects(&self, limit: usize) -> CoreResult<Vec<ProjectRecord>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, path, name, project_type, manifest_path, readme_summary, last_detected_at
             FROM projects
             ORDER BY last_detected_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                project_type: row.get(3)?,
                manifest_path: row.get(4)?,
                readme_summary: row.get(5)?,
                last_detected_at: row.get(6)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Search projects by text using FTS5 trigram index on name, path, and readme_summary.
    /// Falls back to LIKE pattern if trigram produces 0 matches or query is short.
    pub fn search_projects(&self, query: &str, limit: usize) -> CoreResult<Vec<ProjectRecord>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.list_projects(limit);
        }

        let conn = self.reader()?;
        let mut results = Vec::new();

        // 1. Try trigram FTS5 search if length >= 3
        if trimmed.len() >= 3 {
            // Sanitize query for FTS5 trigram phrase
            let clean_query = trimmed.replace('"', "\"\"");
            let fts_expr = format!("\"{}\"", clean_query);

            if let Ok(mut stmt) = conn.prepare_cached(
                "SELECT p.id, p.path, p.name, p.project_type, p.manifest_path, p.readme_summary, p.last_detected_at
                 FROM projects p
                 JOIN projects_fts fts ON fts.rowid = p.id
                 WHERE projects_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
                && let Ok(rows) = stmt.query_map(rusqlite::params![fts_expr, limit as i64], |row| {
                    Ok(ProjectRecord {
                        id: row.get(0)?,
                        path: row.get(1)?,
                        name: row.get(2)?,
                        project_type: row.get(3)?,
                        manifest_path: row.get(4)?,
                        readme_summary: row.get(5)?,
                        last_detected_at: row.get(6)?,
                    })
                }) {
                for r in rows.flatten() {
                    results.push(r);
                }
            }
        }

        // 2. Fallback to LIKE if no FTS results
        if results.is_empty() {
            let like_pattern = format!("%{}%", trimmed);
            let mut stmt = conn.prepare_cached(
                "SELECT id, path, name, project_type, manifest_path, readme_summary, last_detected_at
                 FROM projects
                 WHERE name LIKE ?1 OR path LIKE ?1 OR readme_summary LIKE ?1
                 ORDER BY length(name) ASC, last_detected_at DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![like_pattern, limit as i64], |row| {
                Ok(ProjectRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    project_type: row.get(3)?,
                    manifest_path: row.get(4)?,
                    readme_summary: row.get(5)?,
                    last_detected_at: row.get(6)?,
                })
            })?;
            for r in rows {
                results.push(r?);
            }
        }

        Ok(results)
    }

    /// Get project by path.
    pub fn get_project_by_path(&self, path: &str) -> CoreResult<Option<ProjectRecord>> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare_cached(
            "SELECT id, path, name, project_type, manifest_path, readme_summary, last_detected_at
             FROM projects
             WHERE path = ?1",
        )?;
        let res = stmt
            .query_row([path], |row| {
                Ok(ProjectRecord {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    name: row.get(2)?,
                    project_type: row.get(3)?,
                    manifest_path: row.get(4)?,
                    readme_summary: row.get(5)?,
                    last_detected_at: row.get(6)?,
                })
            })
            .optional()?;
        Ok(res)
    }

    /// Clear all indexed files, chunks, metadata, jobs, problem files, and projects.
    pub fn clear_all_data(&self) -> CoreResult<()> {
        let conn = self.writer();
        conn.execute_batch(
            "DELETE FROM image_tags;
             DELETE FROM image_metadata;
             DELETE FROM chunks;
             DELETE FROM files;
             DELETE FROM problem_files;
             DELETE FROM jobs;
             DELETE FROM projects;
             DELETE FROM files_fts;
             DELETE FROM chunks_fts;
             DELETE FROM projects_fts;
             PRAGMA wal_checkpoint(TRUNCATE);
             VACUUM;",
        )?;
        info!("cleared all database data");
        Ok(())
    }

    /// Truncate all indexed data to prepare for a clean index rebuild.
    pub fn truncate_index_data(&self) -> CoreResult<()> {
        let conn = self.writer();
        conn.execute_batch(
            "DELETE FROM image_tags;
             DELETE FROM image_metadata;
             DELETE FROM chunks;
             DELETE FROM files;
             DELETE FROM problem_files;
             DELETE FROM jobs;
             DELETE FROM projects;
             DELETE FROM files_fts;
             DELETE FROM chunks_fts;
             DELETE FROM projects_fts;
             PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
        info!("truncated index data for rebuild");
        Ok(())
    }
}

/// Applies pragmas to every pooled connection on checkout.
#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;
             PRAGMA foreign_keys = ON;
             PRAGMA temp_store = MEMORY;
             PRAGMA query_only = ON;",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.writer();
        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "6");
    }

    #[test]
    fn test_reader_pool() {
        let db = Database::open_in_memory().unwrap();
        let reader = db.reader().unwrap();
        let tables: Vec<String> = reader
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(!tables.is_empty());
    }

    #[test]
    fn test_project_crud_and_search() {
        let db = Database::open_in_memory().unwrap();
        let prj = ProjectRecord {
            id: 0,
            path: "C:/projects/search-my-computer".into(),
            name: "search-my-computer".into(),
            project_type: "rust".into(),
            manifest_path: Some("C:/projects/search-my-computer/Cargo.toml".into()),
            readme_summary: Some("Local semantic search launcher for laptops".into()),
            last_detected_at: "2026-09-22T00:00:00Z".into(),
        };

        let id = db.upsert_project(&prj).unwrap();
        assert!(id > 0);

        let retrieved = db.get_project_by_path(&prj.path).unwrap().unwrap();
        assert_eq!(retrieved.name, "search-my-computer");
        assert_eq!(retrieved.project_type, "rust");

        // FTS trigram search
        let results = db.search_projects("search", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "search-my-computer");

        // Substring / LIKE search
        let results2 = db.search_projects("computer", 10).unwrap();
        assert_eq!(results2.len(), 1);
    }
}
