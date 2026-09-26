use crate::error::{CoreError, CoreResult};
use rusqlite::Connection;
use tracing::info;

/// Each migration is a (version, description, SQL) tuple.
/// Versions must be sequential starting at 1.
static MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "initial schema",
        r#"
    CREATE TABLE IF NOT EXISTS meta (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS files (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        path            TEXT    NOT NULL UNIQUE,
        parent_dir      TEXT    NOT NULL,
        name            TEXT    NOT NULL,
        ext             TEXT    NOT NULL DEFAULT '',
        size            INTEGER NOT NULL DEFAULT 0,
        mtime           TEXT    NOT NULL,
        ctime           TEXT    NOT NULL,
        kind            TEXT    NOT NULL DEFAULT 'other',
        content_hash    TEXT,
        status          TEXT    NOT NULL DEFAULT 'active',
        last_indexed_at TEXT    NOT NULL,
        error           TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_files_parent_dir ON files(parent_dir);
    CREATE INDEX IF NOT EXISTS idx_files_status ON files(status);
    CREATE INDEX IF NOT EXISTS idx_files_name ON files(name);

    CREATE TABLE IF NOT EXISTS chunks (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
        ordinal INTEGER NOT NULL,
        text    TEXT    NOT NULL,
        start   INTEGER NOT NULL,
        end     INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);

    -- FTS5 with trigram tokenizer for partial filename matching.
    -- Covers camelCase, snake_case, and substring queries.
    CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
        name,
        content = 'files',
        content_rowid = 'id',
        tokenize = 'trigram'
    );

    -- Triggers to keep FTS in sync with the files table.
    CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
        INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
    END;
    CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
        INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
    END;
    CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF name ON files BEGIN
        INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
        INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
    END;

    -- FTS5 for chunk text (standard tokenizer). Created now, filled in Chunk 3.
    CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
        text,
        content = 'chunks',
        content_rowid = 'id',
        tokenize = 'unicode61'
    );

    CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
        INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
    END;
    CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
        INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
    END;
    CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE OF text ON chunks BEGIN
        INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
        INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
    END;

    CREATE TABLE IF NOT EXISTS jobs (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        kind       TEXT    NOT NULL,
        file_id    INTEGER REFERENCES files(id) ON DELETE CASCADE,
        priority   INTEGER NOT NULL DEFAULT 0,
        state      TEXT    NOT NULL DEFAULT 'pending',
        attempts   INTEGER NOT NULL DEFAULT 0,
        error      TEXT,
        created_at TEXT    NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_jobs_state ON jobs(state);
    CREATE INDEX IF NOT EXISTS idx_jobs_priority ON jobs(priority DESC);
    "#,
    ),
    (
        2,
        "add chunk metadata columns",
        r#"
    ALTER TABLE chunks ADD COLUMN page INTEGER;
    ALTER TABLE chunks ADD COLUMN section TEXT;
    ALTER TABLE chunks ADD COLUMN symbol TEXT;
    "#,
    ),
    (
        3,
        "add chunk vectors table for semantic search",
        r#"
    CREATE TABLE IF NOT EXISTS chunk_vectors (
        chunk_id   INTEGER PRIMARY KEY NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
        file_id    INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
        model_id   TEXT    NOT NULL,
        dims       INTEGER NOT NULL,
        text_hash  TEXT    NOT NULL,
        vector     BLOB    NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_chunk_vectors_file_id ON chunk_vectors(file_id);
    CREATE INDEX IF NOT EXISTS idx_chunk_vectors_model_id ON chunk_vectors(model_id);
    "#,
    ),
    (
        4,
        "add projects table and projects_fts for project entity search",
        r#"
    CREATE TABLE IF NOT EXISTS projects (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        path            TEXT    NOT NULL UNIQUE,
        name            TEXT    NOT NULL,
        project_type    TEXT    NOT NULL,
        manifest_path   TEXT,
        readme_summary  TEXT,
        last_detected_at TEXT   NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_projects_path ON projects(path);
    CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);
    CREATE INDEX IF NOT EXISTS idx_projects_type ON projects(project_type);

    CREATE VIRTUAL TABLE IF NOT EXISTS projects_fts USING fts5(
        name,
        path,
        readme_summary,
        content = 'projects',
        content_rowid = 'id',
        tokenize = 'trigram'
    );

    CREATE TRIGGER IF NOT EXISTS projects_ai AFTER INSERT ON projects BEGIN
        INSERT INTO projects_fts(rowid, name, path, readme_summary) VALUES (new.id, new.name, new.path, new.readme_summary);
    END;
    CREATE TRIGGER IF NOT EXISTS projects_ad AFTER DELETE ON projects BEGIN
        INSERT INTO projects_fts(projects_fts, rowid, name, path, readme_summary) VALUES ('delete', old.id, old.name, old.path, old.readme_summary);
    END;
    CREATE TRIGGER IF NOT EXISTS projects_au AFTER UPDATE ON projects BEGIN
        INSERT INTO projects_fts(projects_fts, rowid, name, path, readme_summary) VALUES ('delete', old.id, old.name, old.path, old.readme_summary);
        INSERT INTO projects_fts(rowid, name, path, readme_summary) VALUES (new.id, new.name, new.path, new.readme_summary);
    END;
    "#,
    ),
    (
        5,
        "add image metadata, tags, and vector tables for vision pipeline",
        r#"
    CREATE TABLE IF NOT EXISTS image_metadata (
        file_id       INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
        width         INTEGER NOT NULL,
        height        INTEGER NOT NULL,
        format        TEXT NOT NULL,
        exif_date     TEXT,
        camera_make   TEXT,
        camera_model  TEXT,
        is_screenshot BOOLEAN NOT NULL DEFAULT 0,
        has_qr        BOOLEAN NOT NULL DEFAULT 0,
        qr_count      INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS image_tags (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
        tag         TEXT NOT NULL,
        payload     TEXT,
        created_at  TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_image_tags_file_id ON image_tags(file_id);
    CREATE INDEX IF NOT EXISTS idx_image_tags_tag ON image_tags(tag);

    CREATE TABLE IF NOT EXISTS image_vectors (
        file_id     INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
        model_id    TEXT NOT NULL,
        dims        INTEGER NOT NULL,
        vector      BLOB NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_image_vectors_model_id ON image_vectors(model_id);
    "#,
    ),
    (
        6,
        "add problem_files table for crash resilience and failure tracking",
        r#"
    CREATE TABLE IF NOT EXISTS problem_files (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        file_id        INTEGER REFERENCES files(id) ON DELETE CASCADE,
        path           TEXT NOT NULL UNIQUE,
        error_kind     TEXT NOT NULL,
        error_message  TEXT NOT NULL,
        attempts       INTEGER NOT NULL DEFAULT 1,
        last_failed_at TEXT NOT NULL,
        resolved_at    TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_problem_files_path ON problem_files(path);
    CREATE INDEX IF NOT EXISTS idx_problem_files_error_kind ON problem_files(error_kind);
    "#,
    ),
];

/// Returns the current schema version from the meta table.
/// Returns 0 if the meta table doesn't exist or has no schema_version entry.
pub fn current_version(conn: &Connection) -> CoreResult<i64> {
    // Check if meta table exists at all.
    let table_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        return Ok(0);
    }

    match conn.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(v) => v
            .parse::<i64>()
            .map_err(|e| CoreError::Migration(format!("invalid schema version: {e}"))),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// Run all pending migrations. Returns the number of migrations applied.
pub fn run_migrations(conn: &Connection) -> CoreResult<usize> {
    let current = current_version(conn)?;
    let mut applied = 0;

    for &(version, description, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }

        info!(version, description, "applying migration");

        conn.execute_batch(sql)
            .map_err(|e| CoreError::Migration(format!("migration v{version} failed: {e}")))?;

        // Upsert schema version.
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = ?1",
            [&version.to_string()],
        )?;

        applied += 1;
    }

    if applied > 0 {
        info!(applied, "migrations complete");
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_fresh_migration() {
        let conn = Connection::open_in_memory().unwrap();
        let applied = run_migrations(&conn).unwrap();
        assert_eq!(applied, 6);
        assert_eq!(current_version(&conn).unwrap(), 6);

        // Verify all tables exist.
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"chunks".to_string()));
        assert!(tables.contains(&"chunk_vectors".to_string()));
        assert!(tables.contains(&"jobs".to_string()));
        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"image_metadata".to_string()));
        assert!(tables.contains(&"image_tags".to_string()));
        assert!(tables.contains(&"image_vectors".to_string()));
        assert!(tables.contains(&"problem_files".to_string()));
        assert!(tables.contains(&"meta".to_string()));
    }

    #[test]
    fn test_idempotent_migration() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let applied = run_migrations(&conn).unwrap();
        assert_eq!(applied, 0);
    }

    #[test]
    fn test_fts5_tables_exist() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify FTS tables by querying them.
        conn.execute_batch("INSERT INTO files (path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at) VALUES ('/test/hello.txt', '/test', 'hello.txt', 'txt', 100, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01')")
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files_fts WHERE name MATCH 'hel'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
