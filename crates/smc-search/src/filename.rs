use crate::ranking::{RankingConfig, depth_boost, match_type_boost, recency_boost};
use rusqlite::params;
use serde::Serialize;
use smc_core::db::Database;
use smc_core::error::CoreResult;
use tracing::debug;

/// A single filename search result.
#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub parent_dir: String,
    pub ext: String,
    pub size: i64,
    pub mtime: String,
    pub ctime: String,
    pub kind: String,
    pub score: f64,
}

/// Search filenames by query using FTS5 trigram matching with ranked results.
pub fn search(db: &Database, query: &str, limit: usize) -> CoreResult<Vec<FileResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let reader = db.reader()?;

    // FTS5 trigram tokenizer matches substrings of 3+ characters.
    // For shorter queries, fall back to LIKE prefix matching on the files table.
    let results = if query.len() >= 3 {
        search_fts5(&reader, query, limit)?
    } else {
        search_like_prefix(&reader, query, limit)?
    };

    debug!(
        query_len = query.len(),
        results = results.len(),
        "filename search"
    );
    Ok(results)
}

/// FTS5 trigram search with ranking.
fn search_fts5(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<FileResult>> {
    // The trigram tokenizer lets us pass the query directly for substring matching.
    // We surround with quotes to treat the whole query as a phrase.
    let fts_query = format!("\"{}\"", query.replace('"', "\"\""));

    let mut stmt = conn.prepare(
        "SELECT f.id, f.path, f.name, f.parent_dir, f.ext, f.size, f.mtime, f.ctime, f.kind,
                files_fts.rank AS fts_rank
         FROM files_fts
         JOIN files f ON f.id = files_fts.rowid
         WHERE files_fts MATCH ?1
           AND f.status = 'active'
         ORDER BY files_fts.rank
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![fts_query, limit as i64 * 3], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, f64>(9)?,
        ))
    })?;

    let mut results: Vec<FileResult> = Vec::new();
    for row in rows {
        let (id, path, name, parent_dir, ext, size, mtime, ctime, kind, fts_rank) = row?;

        // FTS5 rank is negative (more negative = better match). Negate for our scoring.
        let base_score = -fts_rank;
        let cfg = RankingConfig::default();
        let score = base_score
            * match_type_boost(&name, query, &cfg)
            * depth_boost(&path, cfg.depth_weight)
            * recency_boost(&mtime, cfg.recency_weight, cfg.recency_half_life_days);

        results.push(FileResult {
            id,
            path,
            name,
            parent_dir,
            ext,
            size,
            mtime,
            ctime,
            kind,
            score,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);

    Ok(results)
}

/// Fallback for short queries (1-2 chars): LIKE prefix match.
fn search_like_prefix(
    conn: &rusqlite::Connection,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<FileResult>> {
    let pattern = format!("{}%", query.replace('%', "\\%").replace('_', "\\_"));

    let mut stmt = conn.prepare(
        "SELECT id, path, name, parent_dir, ext, size, mtime, ctime, kind
         FROM files
         WHERE name LIKE ?1 ESCAPE '\\'
           AND status = 'active'
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![pattern, limit as i64 * 3], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        ))
    })?;

    let mut results: Vec<FileResult> = Vec::new();
    for row in rows {
        let (id, path, name, parent_dir, ext, size, mtime, ctime, kind) = row?;

        let cfg = RankingConfig::default();
        let score = match_type_boost(&name, query, &cfg)
            * depth_boost(&path, cfg.depth_weight)
            * recency_boost(&mtime, cfg.recency_weight, cfg.recency_half_life_days);

        results.push(FileResult {
            id,
            path,
            name,
            parent_dir,
            ext,
            size,
            mtime,
            ctime,
            kind,
            score,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use smc_core::db::Database;

    fn setup_test_db() -> Database {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let db = Database::open(&db_path).unwrap();
        std::mem::forget(tmp);

        // Insert test files.
        let conn = db.writer();
        let files = &[
            (
                "/docs/README.md",
                "/docs",
                "README.md",
                "md",
                1000,
                "document",
            ),
            (
                "/docs/readme_old.md",
                "/docs",
                "readme_old.md",
                "md",
                500,
                "document",
            ),
            ("/src/main.rs", "/src", "main.rs", "rs", 2000, "code"),
            (
                "/src/lib/helper.rs",
                "/src/lib",
                "helper.rs",
                "rs",
                800,
                "code",
            ),
            (
                "/deep/a/b/c/d/e/readme.txt",
                "/deep/a/b/c/d/e",
                "readme.txt",
                "txt",
                100,
                "document",
            ),
            (
                "/projects/SearchEngine.ts",
                "/projects",
                "SearchEngine.ts",
                "ts",
                3000,
                "code",
            ),
            (
                "/projects/search_utils.py",
                "/projects",
                "search_utils.py",
                "py",
                1500,
                "code",
            ),
        ];

        let now = chrono::Utc::now().to_rfc3339();
        for (path, parent, name, ext, size, kind) in files {
            conn.execute(
                "INSERT INTO files (path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, 'active', ?6)",
                params![path, parent, name, ext, size, now, kind],
            )
            .unwrap();
        }
        drop(conn);

        db
    }

    #[test]
    fn test_search_empty_query() {
        let db = setup_test_db();
        let results = search(&db, "", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_trigram_match() {
        let db = setup_test_db();
        let results = search(&db, "readme", 10).unwrap();

        assert!(!results.is_empty());
        // All results should contain "readme" (case-insensitive).
        for r in &results {
            assert!(
                r.name.to_lowercase().contains("readme"),
                "unexpected result: {}",
                r.name
            );
        }
    }

    #[test]
    fn test_exact_match_ranked_higher() {
        let db = setup_test_db();
        let results = search(&db, "README.md", 10).unwrap();

        assert!(!results.is_empty());
        // The exact match "README.md" should be first.
        assert_eq!(results[0].name, "README.md");
    }

    #[test]
    fn test_shallow_path_ranked_higher() {
        let db = setup_test_db();
        let results = search(&db, "readme", 10).unwrap();

        // Find the shallow and deep readme results.
        let shallow_idx = results.iter().position(|r| r.path == "/docs/README.md");
        let deep_idx = results
            .iter()
            .position(|r| r.path == "/deep/a/b/c/d/e/readme.txt");

        if let (Some(s), Some(d)) = (shallow_idx, deep_idx) {
            assert!(s < d, "shallow path should rank before deep path");
        }
    }

    #[test]
    fn test_search_camel_case() {
        let db = setup_test_db();
        let results = search(&db, "SearchEngine", 10).unwrap();

        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.name == "SearchEngine.ts"));
    }

    #[test]
    fn test_search_snake_case() {
        let db = setup_test_db();
        let results = search(&db, "search_utils", 10).unwrap();

        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.name == "search_utils.py"));
    }

    #[test]
    fn test_short_query_fallback() {
        let db = setup_test_db();
        // 2-char query falls back to LIKE prefix match.
        let results = search(&db, "RE", 10).unwrap();
        // Should find README.md and readme_old.md (prefix match on name).
        assert!(!results.is_empty());
    }
}
