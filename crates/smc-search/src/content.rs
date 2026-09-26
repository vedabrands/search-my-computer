use crate::ranking::{RankingConfig, depth_boost, recency_boost};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use smc_core::db::Database;
use smc_core::error::CoreResult;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMatch {
    pub file_id: i64,
    pub chunk_id: i64,
    pub path: String,
    pub name: String,
    pub parent_dir: String,
    pub ext: String,
    pub size: i64,
    pub mtime: String,
    pub ctime: String,
    pub kind: String,
    pub score: f64,
    pub snippet: String,
    pub page: Option<usize>,
    pub section: Option<String>,
    pub symbol: Option<String>,
}

/// Search chunk contents using FTS5 with BM25 ranking and snippet generation.
pub fn search_content(db: &Database, query: &str, limit: usize) -> CoreResult<Vec<ContentMatch>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let fts_query = sanitize_fts5_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let reader = db.reader()?;

    let mut stmt = reader.prepare(
        "SELECT c.id, c.file_id, c.page, c.section, c.symbol,
                snippet(chunks_fts, 0, '<mark>', '</mark>', '...', 24) AS snippet_text,
                chunks_fts.rank AS bm25_rank,
                f.path, f.name, f.parent_dir, f.ext, f.size, f.mtime, f.ctime, f.kind
         FROM chunks_fts
         JOIN chunks c ON c.id = chunks_fts.rowid
         JOIN files f ON f.id = c.file_id
         WHERE chunks_fts MATCH ?1
           AND f.status = 'active'
         ORDER BY chunks_fts.rank
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![fts_query, (limit * 3) as i64], |row| {
        let chunk_id: i64 = row.get(0)?;
        let file_id: i64 = row.get(1)?;
        let page: Option<i64> = row.get(2)?;
        let section: Option<String> = row.get(3)?;
        let symbol: Option<String> = row.get(4)?;
        let snippet: String = row.get(5)?;
        let bm25_rank: f64 = row.get(6)?;
        let path: String = row.get(7)?;
        let name: String = row.get(8)?;
        let parent_dir: String = row.get(9)?;
        let ext: String = row.get(10)?;
        let size: i64 = row.get(11)?;
        let mtime: String = row.get(12)?;
        let ctime: String = row.get(13)?;
        let kind: String = row.get(14)?;

        Ok((
            chunk_id, file_id, page, section, symbol, snippet, bm25_rank, path, name, parent_dir,
            ext, size, mtime, ctime, kind,
        ))
    })?;

    let mut matches = Vec::new();
    for row in rows {
        let (
            chunk_id,
            file_id,
            page,
            section,
            symbol,
            snippet,
            bm25_rank,
            path,
            name,
            parent_dir,
            ext,
            size,
            mtime,
            ctime,
            kind,
        ) = match row {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "error reading content match row");
                continue;
            }
        };

        // Negative rank: more negative is a stronger BM25 match.
        let base_score = -bm25_rank;
        let cfg = RankingConfig::default();
        let score = base_score
            * depth_boost(&path, cfg.depth_weight)
            * recency_boost(&mtime, cfg.recency_weight, cfg.recency_half_life_days);

        matches.push(ContentMatch {
            file_id,
            chunk_id,
            path,
            name,
            parent_dir,
            ext,
            size,
            mtime,
            ctime,
            kind,
            score,
            snippet,
            page: page.map(|p| p as usize),
            section,
            symbol,
        });
    }

    Ok(matches)
}

/// Sanitize user input for SQLite FTS5 query format.
pub fn sanitize_fts5_query(query: &str) -> String {
    let words: Vec<&str> = query.split_whitespace().filter(|w| !w.is_empty()).collect();

    if words.is_empty() {
        return String::new();
    }

    let mut terms = Vec::new();
    for word in words {
        // Strip FTS5 operators and punctuation that could cause syntax errors
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();

        if !clean.is_empty() {
            // Append prefix search wildcard for each term
            terms.push(format!("\"{}\"*", clean));
        }
    }

    if terms.is_empty() {
        return String::new();
    }

    terms.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use smc_core::db::Database;

    fn setup_content_test_db() -> Database {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("content_test.db");
        let db = Database::open(&db_path).unwrap();
        std::mem::forget(tmp);

        {
            let conn = db.writer();

            // 1. Insert a file
            conn.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (1, '/docs/guide.pdf', '/docs', 'guide.pdf', 'pdf', 5000, datetime('now'), datetime('now'), 'pdf', 'active', datetime('now'))",
                [],
            ).unwrap();

            // 2. Insert chunks (triggers automatically populate chunks_fts)
            conn.execute(
                "INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (1, 1, 0, 'Local semantic search launcher with complete privacy and zero telemetry.', 1, 'Overview', NULL, 0, 80)",
                [],
            ).unwrap();
        }

        db
    }

    #[test]
    fn test_search_content_bm25_and_snippet() {
        let db = setup_content_test_db();
        let results = search_content(&db, "privacy telemetry", 10).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "guide.pdf");
        assert_eq!(results[0].page, Some(1));
        assert_eq!(results[0].section, Some("Overview".to_string()));
        assert!(results[0].snippet.contains("<mark>"));
    }

    #[test]
    fn test_search_content_empty_query() {
        let db = setup_content_test_db();
        let results = search_content(&db, "", 10).unwrap();
        assert!(results.is_empty());
    }
}
