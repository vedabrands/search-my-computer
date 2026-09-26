use crate::content::search_content;
use crate::filename::search as search_filename;
use crate::ranking::{
    QueryIntent, RankingConfig, character_trigram_jaccard, classify_query_intent, depth_boost,
    file_type_prior, location_boost, match_type_boost, recency_boost, rrf_score, tag_boost,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use smc_core::db::Database;
use smc_core::error::CoreResult;
use smc_embed::embedder::Embedder;
use smc_embed::vector_index::VectorIndex;
use smc_nlq::ParsedQuery;
use std::collections::{HashMap, HashSet};

/// Secondary non-duplicate chunk match for a search result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkMatchSnippet {
    pub chunk_id: i64,
    pub snippet: String,
    pub page: Option<usize>,
    pub section: Option<String>,
    pub symbol: Option<String>,
    pub score: f64,
}

/// Unified search result item returned to UI and consumers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub parent_dir: String,
    pub ext: String,
    pub size: i64,
    pub mtime: String,
    pub kind: String,
    pub score: f64,
    pub match_type: String, // "filename" | "content" | "semantic" | "hybrid" | "attribute" | "visual" | "tag"
    pub snippet: Option<String>,
    pub page: Option<usize>,
    pub section: Option<String>,
    pub symbol: Option<String>,
    pub matches: Vec<ChunkMatchSnippet>,
    pub tag: Option<String>,
    pub masked_payload: Option<String>,
    pub thumbnail_path: Option<String>,
    pub is_screenshot: bool,
}

#[derive(Debug, Clone)]
struct CandidateFile {
    id: i64,
    path: String,
    name: String,
    parent_dir: String,
    ext: String,
    size: i64,
    mtime: String,
    ctime: String,
    kind: String,
    filename_rank: Option<usize>,
    content_rank: Option<usize>,
    vector_rank: Option<usize>,
    image_tag_rank: Option<usize>,
    image_vector_rank: Option<usize>,
    tag: Option<String>,
    masked_payload: Option<String>,
    is_screenshot: bool,
    raw_chunks: Vec<ChunkCandidate>,
}

#[derive(Debug, Clone)]
struct ChunkCandidate {
    chunk_id: i64,
    text: String,
    snippet: String,
    page: Option<usize>,
    section: Option<String>,
    symbol: Option<String>,
    score: f64,
}

/// Hit returned from matching tags in `image_tags` / `image_metadata`.
#[derive(Debug, Clone)]
pub struct ImageTagHit {
    pub file_id: i64,
    pub path: String,
    pub name: String,
    pub parent_dir: String,
    pub ext: String,
    pub size: i64,
    pub mtime: String,
    pub ctime: String,
    pub kind: String,
    pub tag: String,
    pub payload: Option<String>,
    pub is_screenshot: bool,
}

/// Hit returned from cosine matching in `image_vectors`.
#[derive(Debug, Clone)]
pub struct ImageVectorHit {
    pub file_id: i64,
    pub path: String,
    pub name: String,
    pub parent_dir: String,
    pub ext: String,
    pub size: i64,
    pub mtime: String,
    pub ctime: String,
    pub kind: String,
    pub score: f32,
    pub is_screenshot: bool,
}

/// Helper: parse SQLite datetime strings (RFC3339 or ISO formats).
fn parse_db_datetime(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{DateTime, TimeZone, Utc};
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }
    None
}

/// Executes a natural language query aware hybrid search.
pub fn hybrid_search(db: &Database, query: &str, limit: usize) -> CoreResult<Vec<SearchResult>> {
    hybrid_search_nlq(db, query, limit)
}

/// Executes hybrid search with a customized ranking configuration (no vector search).
pub fn hybrid_search_with_config(
    db: &Database,
    query: &str,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let parsed = smc_nlq::parse_query(query);
    hybrid_search_with_parsed_query(db, None, None, &parsed, limit, config)
}

/// Executes NLQ hybrid search with default ranking config.
pub fn hybrid_search_nlq(
    db: &Database,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<SearchResult>> {
    let parsed = smc_nlq::parse_query(query);
    hybrid_search_with_parsed_query(db, None, None, &parsed, limit, &RankingConfig::default())
}

/// Executes full NLQ hybrid search combining all 3 text streams with custom embedder and vector index.
pub fn hybrid_search_nlq_full(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    query: &str,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let parsed = smc_nlq::parse_query(query);
    hybrid_search_with_parsed_query_and_vision(
        db,
        embedder,
        vector_index,
        None,
        &parsed,
        limit,
        config,
    )
}

/// Executes full hybrid search combining all 3 text streams.
pub fn hybrid_search_full(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    query: &str,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let parsed = smc_nlq::parse_query(query);
    hybrid_search_with_parsed_query_and_vision(
        db,
        embedder,
        vector_index,
        None,
        &parsed,
        limit,
        config,
    )
}

/// Executes full NLQ hybrid search combining text streams and vision streams.
pub fn hybrid_search_nlq_vision(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    clip_engine: Option<&smc_vision::clip::ClipEngine>,
    query: &str,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let parsed = smc_nlq::parse_query(query);
    hybrid_search_with_parsed_query_and_vision(
        db,
        embedder,
        vector_index,
        clip_engine,
        &parsed,
        limit,
        config,
    )
}

/// Executes hybrid search driven by a pre-parsed NLQ query with hard filters and soft multipliers.
pub fn hybrid_search_with_parsed_query(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    parsed: &ParsedQuery,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    hybrid_search_with_parsed_query_and_vision(
        db,
        embedder,
        vector_index,
        None,
        parsed,
        limit,
        config,
    )
}

/// Executes hybrid search combining text streams and vision streams.
pub fn hybrid_search_with_parsed_query_and_vision(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    clip_engine: Option<&smc_vision::clip::ClipEngine>,
    parsed: &ParsedQuery,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let clean_text = parsed.text.trim();

    // If query text is empty, but we have filters (e.g. "pdf from last week", "screenshots in downloads")
    if clean_text.is_empty() {
        if parsed.has_filters() {
            return search_attribute_only(db, parsed, limit, config);
        } else {
            return Ok(Vec::new());
        }
    }

    if limit == 0 {
        return Ok(Vec::new());
    }

    let search_limit = (limit * 4).max(30);

    // Run retrievals in parallel across database reader connections using std::thread::scope
    let (filename_hits, content_hits, vector_hits, image_tag_hits, image_vector_hits) =
        std::thread::scope(|s| {
            let handle_fn =
                s.spawn(|| search_filename(db, clean_text, search_limit).unwrap_or_default());
            let handle_cnt =
                s.spawn(|| search_content(db, clean_text, search_limit).unwrap_or_default());
            let handle_vec = s.spawn(|| {
                if let (Some(emb), Some(v_idx)) = (embedder, vector_index) {
                    retrieve_vector_hits(db, emb, v_idx, clean_text, search_limit)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            });
            let handle_tag = s.spawn(|| {
                if !parsed.tag_filters.is_empty()
                    || parsed.is_screenshot_query
                    || clean_text.to_lowercase().contains("qr")
                {
                    search_image_tags(db, &parsed.tag_filters, clean_text, search_limit)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            });
            let handle_img_vec = s.spawn(|| {
                if parsed.is_visual_query || clip_engine.is_some() {
                    search_image_vectors(db, clip_engine, clean_text, search_limit)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            });

            (
                handle_fn.join().unwrap_or_default(),
                handle_cnt.join().unwrap_or_default(),
                handle_vec.join().unwrap_or_default(),
                handle_tag.join().unwrap_or_default(),
                handle_img_vec.join().unwrap_or_default(),
            )
        });

    // 1. Group hits by file_id and assign stream ranks
    let mut candidates: HashMap<i64, CandidateFile> = HashMap::new();

    // Filename stream ranks
    for (idx, f) in filename_hits.into_iter().enumerate() {
        let rank = idx + 1;
        candidates
            .entry(f.id)
            .and_modify(|c| c.filename_rank = Some(rank))
            .or_insert_with(|| CandidateFile {
                id: f.id,
                path: f.path,
                name: f.name,
                parent_dir: f.parent_dir,
                ext: f.ext,
                size: f.size,
                mtime: f.mtime,
                ctime: f.ctime,
                kind: f.kind,
                filename_rank: Some(rank),
                content_rank: None,
                vector_rank: None,
                image_tag_rank: None,
                image_vector_rank: None,
                tag: None,
                masked_payload: None,
                is_screenshot: false,
                raw_chunks: Vec::new(),
            });
    }

    // Content BM25 stream ranks (grouped by unique file_id)
    let mut seen_content_files = HashSet::new();
    let mut content_file_rank = 1;
    for c in content_hits {
        let is_new = seen_content_files.insert(c.file_id);
        let rank = if is_new {
            let r = content_file_rank;
            content_file_rank += 1;
            Some(r)
        } else {
            None
        };

        candidates
            .entry(c.file_id)
            .and_modify(|entry| {
                if is_new && entry.content_rank.is_none() {
                    entry.content_rank = rank;
                }
                entry.raw_chunks.push(ChunkCandidate {
                    chunk_id: c.chunk_id,
                    text: c.snippet.replace("<mark>", "").replace("</mark>", ""),
                    snippet: c.snippet.clone(),
                    page: c.page,
                    section: c.section.clone(),
                    symbol: c.symbol.clone(),
                    score: c.score,
                });
            })
            .or_insert_with(|| CandidateFile {
                id: c.file_id,
                path: c.path,
                name: c.name,
                parent_dir: c.parent_dir,
                ext: c.ext,
                size: c.size,
                mtime: c.mtime,
                ctime: c.ctime,
                kind: c.kind,
                filename_rank: None,
                content_rank: rank,
                vector_rank: None,
                image_tag_rank: None,
                image_vector_rank: None,
                tag: None,
                masked_payload: None,
                is_screenshot: false,
                raw_chunks: vec![ChunkCandidate {
                    chunk_id: c.chunk_id,
                    text: c.snippet.replace("<mark>", "").replace("</mark>", ""),
                    snippet: c.snippet,
                    page: c.page,
                    section: c.section,
                    symbol: c.symbol,
                    score: c.score,
                }],
            });
    }

    // Vector stream ranks (grouped by unique file_id)
    let mut seen_vector_files = HashSet::new();
    let mut vector_file_rank = 1;
    for v in vector_hits {
        let is_new = seen_vector_files.insert(v.file_id);
        let rank = if is_new {
            let r = vector_file_rank;
            vector_file_rank += 1;
            Some(r)
        } else {
            None
        };

        candidates
            .entry(v.file_id)
            .and_modify(|entry| {
                if is_new && entry.vector_rank.is_none() {
                    entry.vector_rank = rank;
                }
                entry.raw_chunks.push(ChunkCandidate {
                    chunk_id: v.chunk_id,
                    text: v.text.clone(),
                    snippet: v.snippet.clone(),
                    page: v.page,
                    section: v.section.clone(),
                    symbol: v.symbol.clone(),
                    score: v.score as f64,
                });
            })
            .or_insert_with(|| CandidateFile {
                id: v.file_id,
                path: v.path,
                name: v.name,
                parent_dir: v.parent_dir,
                ext: v.ext,
                size: v.size,
                mtime: v.mtime,
                ctime: v.ctime,
                kind: v.kind,
                filename_rank: None,
                content_rank: None,
                vector_rank: rank,
                image_tag_rank: None,
                image_vector_rank: None,
                tag: None,
                masked_payload: None,
                is_screenshot: false,
                raw_chunks: vec![ChunkCandidate {
                    chunk_id: v.chunk_id,
                    text: v.text,
                    snippet: v.snippet,
                    page: v.page,
                    section: v.section,
                    symbol: v.symbol,
                    score: v.score as f64,
                }],
            });
    }

    // Image tag stream ranks
    let mut seen_tag_files = HashSet::new();
    let mut tag_file_rank = 1;
    for t in image_tag_hits {
        let is_new = seen_tag_files.insert(t.file_id);
        let rank = if is_new {
            let r = tag_file_rank;
            tag_file_rank += 1;
            Some(r)
        } else {
            None
        };

        let tag_clone = t.tag.clone();
        let payload_clone = t.payload.clone();
        let is_screenshot_val = t.is_screenshot;

        candidates
            .entry(t.file_id)
            .and_modify(|entry| {
                if is_new && entry.image_tag_rank.is_none() {
                    entry.image_tag_rank = rank;
                }
                if entry.tag.is_none() {
                    entry.tag = Some(tag_clone.clone());
                    entry.masked_payload = payload_clone.clone();
                }
                if is_screenshot_val {
                    entry.is_screenshot = true;
                }
            })
            .or_insert_with(|| CandidateFile {
                id: t.file_id,
                path: t.path,
                name: t.name,
                parent_dir: t.parent_dir,
                ext: t.ext,
                size: t.size,
                mtime: t.mtime,
                ctime: t.ctime,
                kind: t.kind,
                filename_rank: None,
                content_rank: None,
                vector_rank: None,
                image_tag_rank: rank,
                image_vector_rank: None,
                tag: Some(tag_clone),
                masked_payload: payload_clone,
                is_screenshot: is_screenshot_val,
                raw_chunks: Vec::new(),
            });
    }

    // Image visual vector stream ranks
    let mut seen_img_vec_files = HashSet::new();
    let mut img_vec_file_rank = 1;
    for iv in image_vector_hits {
        let is_new = seen_img_vec_files.insert(iv.file_id);
        let rank = if is_new {
            let r = img_vec_file_rank;
            img_vec_file_rank += 1;
            Some(r)
        } else {
            None
        };

        let is_screenshot_val = iv.is_screenshot;

        candidates
            .entry(iv.file_id)
            .and_modify(|entry| {
                if is_new && entry.image_vector_rank.is_none() {
                    entry.image_vector_rank = rank;
                }
                if is_screenshot_val {
                    entry.is_screenshot = true;
                }
            })
            .or_insert_with(|| CandidateFile {
                id: iv.file_id,
                path: iv.path,
                name: iv.name,
                parent_dir: iv.parent_dir,
                ext: iv.ext,
                size: iv.size,
                mtime: iv.mtime,
                ctime: iv.ctime,
                kind: iv.kind,
                filename_rank: None,
                content_rank: None,
                vector_rank: None,
                image_tag_rank: None,
                image_vector_rank: rank,
                tag: None,
                masked_payload: None,
                is_screenshot: is_screenshot_val,
                raw_chunks: Vec::new(),
            });
    }

    // 2. Score candidate files
    let intent = classify_query_intent(clean_text);
    let mut results = score_candidates(&candidates, parsed, clean_text, intent, config, true);

    // Graceful fallback: if strict metadata filters eliminated all candidates,
    // evaluate without hard pruning so semantic content matches are still surfaced.
    if results.is_empty() && parsed.has_filters() && !candidates.is_empty() {
        results = score_candidates(&candidates, parsed, clean_text, intent, config, false);
    }

    if results.len() > limit {
        results.truncate(limit);
    }

    Ok(results)
}

/// Search `image_tags` and `image_metadata` for tag matches and keywords.
pub fn search_image_tags(
    db: &Database,
    tag_filters: &[String],
    query_text: &str,
    limit: usize,
) -> CoreResult<Vec<ImageTagHit>> {
    let reader = db.reader()?;
    let mut sql = "SELECT t.file_id, t.tag, t.payload, f.path, f.name, f.parent_dir, f.ext, f.size, f.mtime, f.ctime, f.kind, COALESCE(m.is_screenshot, 0)
                   FROM image_tags t
                   JOIN files f ON t.file_id = f.id
                   LEFT JOIN image_metadata m ON t.file_id = m.file_id
                   WHERE f.status = 'active'".to_string();

    let mut conditions = Vec::new();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if !tag_filters.is_empty() {
        let mut filter_conds = Vec::new();
        for tf in tag_filters {
            if tf == "qr:*" {
                filter_conds.push("t.tag LIKE 'qr:%'");
            } else if tf == "type:screenshot" {
                filter_conds.push("(t.tag = 'type:screenshot' OR m.is_screenshot = 1)");
            } else if tf == "type:photo" {
                filter_conds.push("t.tag = 'type:photo'");
            } else {
                filter_conds.push("t.tag = ?");
                params_vec.push(Box::new(tf.clone()));
            }
        }
        if !filter_conds.is_empty() {
            conditions.push(format!("({})", filter_conds.join(" OR ")));
        }
    } else if !query_text.trim().is_empty() {
        let q = format!("%{}%", query_text.trim());
        conditions.push("(t.tag LIKE ? OR t.payload LIKE ?)".to_string());
        params_vec.push(Box::new(q.clone()));
        params_vec.push(Box::new(q));
    }

    if !conditions.is_empty() {
        sql.push_str(" AND ");
        sql.push_str(&conditions.join(" AND "));
    }

    sql.push_str(" ORDER BY t.id DESC LIMIT ?");
    params_vec.push(Box::new(limit as i64));

    let mut stmt = match reader.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(error = %e, "image_tags search failed or table missing");
            return Ok(Vec::new());
        }
    };

    let params_slice: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_slice.as_slice(), |row| {
        let is_screenshot_i64: i64 = row.get(11)?;
        Ok(ImageTagHit {
            file_id: row.get(0)?,
            tag: row.get(1)?,
            payload: row.get(2)?,
            path: row.get(3)?,
            name: row.get(4)?,
            parent_dir: row.get(5)?,
            ext: row.get(6)?,
            size: row.get(7)?,
            mtime: row.get(8)?,
            ctime: row.get(9)?,
            kind: row.get(10)?,
            is_screenshot: is_screenshot_i64 != 0,
        })
    })?;

    let mut hits = Vec::new();
    for h in rows.flatten() {
        hits.push(h);
    }
    Ok(hits)
}

/// Search `image_vectors` using CLIP text tower embedding dot products.
pub fn search_image_vectors(
    db: &Database,
    clip_engine: Option<&smc_vision::clip::ClipEngine>,
    query_text: &str,
    limit: usize,
) -> CoreResult<Vec<ImageVectorHit>> {
    let engine = match clip_engine {
        Some(e) if e.is_available() => e,
        _ => return Ok(Vec::new()),
    };

    let q_emb = match engine.embed_text(query_text) {
        Ok(Some(v)) => v,
        _ => return Ok(Vec::new()),
    };

    let reader = db.reader()?;
    let sql = "SELECT v.file_id, v.vector, f.path, f.name, f.parent_dir, f.ext, f.size, f.mtime, f.ctime, f.kind, COALESCE(m.is_screenshot, 0)
               FROM image_vectors v
               JOIN files f ON v.file_id = f.id
               LEFT JOIN image_metadata m ON v.file_id = m.file_id
               WHERE f.status = 'active'";

    let mut stmt = match reader.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let rows = stmt.query_map([], |row| {
        let file_id: i64 = row.get(0)?;
        let blob: Vec<u8> = row.get(1)?;
        let path: String = row.get(2)?;
        let name: String = row.get(3)?;
        let parent_dir: String = row.get(4)?;
        let ext: String = row.get(5)?;
        let size: i64 = row.get(6)?;
        let mtime: String = row.get(7)?;
        let ctime: String = row.get(8)?;
        let kind: String = row.get(9)?;
        let is_screenshot_i64: i64 = row.get(10)?;
        Ok((
            file_id,
            blob,
            path,
            name,
            parent_dir,
            ext,
            size,
            mtime,
            ctime,
            kind,
            is_screenshot_i64 != 0,
        ))
    })?;

    let mut scored_hits = Vec::new();
    for (file_id, blob, path, name, parent_dir, ext, size, mtime, ctime, kind, is_screenshot) in
        rows.flatten()
    {
        if blob.len() % std::mem::size_of::<half::f16>() != 0 {
            continue;
        }
        let f16_slice: &[half::f16] = unsafe {
            std::slice::from_raw_parts(
                blob.as_ptr() as *const half::f16,
                blob.len() / std::mem::size_of::<half::f16>(),
            )
        };
        let mut dot = 0.0f32;
        let n = q_emb.len().min(f16_slice.len());
        for i in 0..n {
            dot += q_emb[i].to_f32() * f16_slice[i].to_f32();
        }
        if dot > 0.0 {
            scored_hits.push(ImageVectorHit {
                file_id,
                path,
                name,
                parent_dir,
                ext,
                size,
                mtime,
                ctime,
                kind,
                score: dot,
                is_screenshot,
            });
        }
    }

    scored_hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if scored_hits.len() > limit {
        scored_hits.truncate(limit);
    }

    Ok(scored_hits)
}

fn score_candidates(
    candidates: &HashMap<i64, CandidateFile>,
    parsed: &ParsedQuery,
    clean_text: &str,
    intent: QueryIntent,
    config: &RankingConfig,
    strict_filters: bool,
) -> Vec<SearchResult> {
    let mut results = Vec::with_capacity(candidates.len());

    for candidate in candidates.values() {
        if strict_filters {
            // Hard filter: file_types
            if !parsed.file_types.is_empty() {
                let ext_lower = candidate.ext.to_lowercase();
                if !parsed
                    .file_types
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(&ext_lower))
                {
                    continue;
                }
            }

            // Hard filter: screenshot intent
            if parsed.is_screenshot_query {
                let is_image = matches!(
                    candidate.ext.to_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "bmp"
                ) || candidate.kind == "image"
                    || candidate.kind == "screenshot";
                let name_lower = candidate.name.to_lowercase();
                let path_lower = candidate.path.to_lowercase();
                let is_screenshot_named = candidate.is_screenshot
                    || name_lower.contains("screenshot")
                    || name_lower.contains("screen shot")
                    || name_lower.contains("screen_shot")
                    || name_lower.contains("capture")
                    || path_lower.contains("screenshot")
                    || path_lower.contains("screen shot")
                    || path_lower.contains("screenshots");
                if !is_image && !is_screenshot_named {
                    continue;
                }
            }

            // Hard filter: date ranges (after / before)
            if parsed.after.is_some() || parsed.before.is_some() {
                let time_str = if parsed.use_ctime && !candidate.ctime.is_empty() {
                    &candidate.ctime
                } else {
                    &candidate.mtime
                };

                if let Some(file_dt) = parse_db_datetime(time_str) {
                    if let Some(after_dt) = parsed.after
                        && file_dt < after_dt
                    {
                        continue;
                    }
                    if let Some(before_dt) = parsed.before
                        && file_dt > before_dt
                    {
                        continue;
                    }
                }
            }
        }

        // Compute base RRF score across streams
        let mut rrf_total = 0.0;
        let mut streams_matched = 0;

        if let Some(r) = candidate.filename_rank {
            rrf_total += rrf_score(r, config.rrf_k, config.weight_filename);
            streams_matched += 1;
        }
        if let Some(r) = candidate.content_rank {
            rrf_total += rrf_score(r, config.rrf_k, config.weight_content);
            streams_matched += 1;
        }
        if let Some(r) = candidate.vector_rank {
            rrf_total += rrf_score(r, config.rrf_k, config.weight_vector);
            streams_matched += 1;
        }
        if let Some(r) = candidate.image_tag_rank {
            rrf_total += rrf_score(r, config.rrf_k, config.weight_image_tag);
            streams_matched += 1;
        }
        if let Some(r) = candidate.image_vector_rank {
            rrf_total += rrf_score(r, config.rrf_k, config.weight_image_vector);
            streams_matched += 1;
        }

        // Determine match type classification
        let match_type = if streams_matched >= 2 {
            "hybrid".to_string()
        } else if candidate.filename_rank.is_some() {
            "filename".to_string()
        } else if candidate.content_rank.is_some() {
            "content".to_string()
        } else if candidate.image_tag_rank.is_some() {
            "tag".to_string()
        } else if candidate.image_vector_rank.is_some() {
            "visual".to_string()
        } else {
            "semantic".to_string()
        };

        // Multipliers
        let fn_multiplier = if candidate.filename_rank.is_some() {
            match_type_boost(&candidate.name, clean_text, config)
        } else {
            1.0
        };
        let depth_mult = depth_boost(&candidate.path, config.depth_weight);
        let recency_mult = recency_boost(
            &candidate.mtime,
            config.recency_weight,
            config.recency_half_life_days,
        );
        let file_type_mult = file_type_prior(
            intent,
            &candidate.ext,
            &candidate.kind,
            config.file_type_prior_weight,
        );
        let loc_mult = location_boost(
            &candidate.path,
            &parsed.location_hints,
            config.location_boost_weight,
        );
        let tag_mult = tag_boost(candidate.tag.is_some(), config.tag_boost_weight);

        let multiplied_score = rrf_total
            * fn_multiplier
            * depth_mult
            * recency_mult
            * file_type_mult
            * loc_mult
            * tag_mult;

        // Process chunk matches with intra-file diversity & deduplication
        let mut chunks_sorted = candidate.raw_chunks.clone();
        chunks_sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut primary_snippet = None;
        let mut primary_page = None;
        let mut primary_section = None;
        let mut primary_symbol = None;
        let mut diverse_matches: Vec<ChunkMatchSnippet> = Vec::new();
        let mut accepted_texts: Vec<String> = Vec::new();

        for chunk in chunks_sorted {
            if primary_snippet.is_none() {
                primary_snippet = Some(chunk.snippet);
                primary_page = chunk.page;
                primary_section = chunk.section;
                primary_symbol = chunk.symbol;
                accepted_texts.push(chunk.text);
            } else if diverse_matches.len() < config.max_additional_matches {
                let is_duplicate = accepted_texts.iter().any(|accepted| {
                    character_trigram_jaccard(accepted, &chunk.text)
                        >= config.chunk_similarity_threshold
                });

                if !is_duplicate {
                    accepted_texts.push(chunk.text);
                    diverse_matches.push(ChunkMatchSnippet {
                        chunk_id: chunk.chunk_id,
                        snippet: chunk.snippet,
                        page: chunk.page,
                        section: chunk.section,
                        symbol: chunk.symbol,
                        score: chunk.score,
                    });
                }
            }
        }

        let multi_chunk_bonus = config.multi_chunk_bonus * diverse_matches.len() as f64;
        let final_score = multiplied_score + multi_chunk_bonus;

        let is_image_ext = matches!(
            candidate.ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
        ) || candidate.kind == "image"
            || candidate.kind == "screenshot";

        let thumbnail_path = if is_image_ext {
            let mut hasher = Sha256::new();
            hasher.update(candidate.path.as_bytes());
            let key = format!("{:x}", hasher.finalize());
            if let Ok(app_data_dir) = smc_core::config::AppConfig::data_dir() {
                let thumb_file = app_data_dir.join("thumbnails").join(format!("{}.jpg", key));
                if thumb_file.exists() {
                    Some(thumb_file.to_string_lossy().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let is_screenshot = candidate.is_screenshot
            || candidate.name.to_lowercase().contains("screenshot")
            || candidate.path.to_lowercase().contains("screenshot");

        results.push(SearchResult {
            id: candidate.id,
            path: candidate.path.clone(),
            name: candidate.name.clone(),
            parent_dir: candidate.parent_dir.clone(),
            ext: candidate.ext.clone(),
            size: candidate.size,
            mtime: candidate.mtime.clone(),
            kind: candidate.kind.clone(),
            score: final_score,
            match_type,
            snippet: primary_snippet,
            page: primary_page,
            section: primary_section,
            symbol: primary_symbol,
            matches: diverse_matches,
            tag: candidate.tag.clone(),
            masked_payload: candidate.masked_payload.clone(),
            thumbnail_path,
            is_screenshot,
        });
    }

    // Sort descending by score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

/// Fallback search when query contains only filters and no content keywords.
fn search_attribute_only(
    db: &Database,
    parsed: &ParsedQuery,
    limit: usize,
    config: &RankingConfig,
) -> CoreResult<Vec<SearchResult>> {
    let reader = db.reader()?;
    let order_col = if parsed.use_ctime {
        "f.ctime"
    } else {
        "f.mtime"
    };
    let query_sql = format!(
        "SELECT f.id, f.path, f.name, f.parent_dir, f.ext, f.size, f.mtime, f.ctime, f.kind,
                COALESCE(m.is_screenshot, 0), t.tag, t.payload
         FROM files f
         LEFT JOIN image_metadata m ON f.id = m.file_id
         LEFT JOIN image_tags t ON f.id = t.file_id
         WHERE f.status = 'active'
         ORDER BY {} DESC
         LIMIT 1000",
        order_col
    );

    let mut stmt = reader.prepare(&query_sql)?;
    let rows = stmt.query_map([], |row| {
        let is_screenshot_i64: i64 = row.get(9)?;
        let tag: Option<String> = row.get(10)?;
        let payload: Option<String> = row.get(11)?;
        Ok((
            CandidateFile {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                parent_dir: row.get(3)?,
                ext: row.get(4)?,
                size: row.get(5)?,
                mtime: row.get(6)?,
                ctime: row.get(7)?,
                kind: row.get(8)?,
                filename_rank: None,
                content_rank: None,
                vector_rank: None,
                image_tag_rank: None,
                image_vector_rank: None,
                tag: tag.clone(),
                masked_payload: payload,
                is_screenshot: is_screenshot_i64 != 0,
                raw_chunks: Vec::new(),
            },
            tag,
            is_screenshot_i64 != 0,
        ))
    })?;

    let mut results = Vec::new();
    let mut seen_ids = HashSet::new();

    for row in rows {
        let (c, tag_opt, is_screenshot_db) = match row {
            Ok(tuple) => tuple,
            Err(_) => continue,
        };

        if !seen_ids.insert(c.id) {
            continue;
        }

        // Tag filter checks
        if !parsed.tag_filters.is_empty() {
            let matches_tags = parsed.tag_filters.iter().all(|tf| {
                if tf == "type:screenshot" {
                    is_screenshot_db
                        || c.name.to_lowercase().contains("screenshot")
                        || c.path.to_lowercase().contains("screenshot")
                } else if tf == "type:photo" {
                    tag_opt.as_deref() == Some("type:photo")
                } else if tf == "qr:*" {
                    tag_opt.as_deref().is_some_and(|t| t.starts_with("qr:"))
                } else {
                    tag_opt.as_deref() == Some(tf.as_str())
                }
            });
            if !matches_tags {
                continue;
            }
        }

        // File type filter
        if !parsed.file_types.is_empty() {
            let ext_lower = c.ext.to_lowercase();
            if !parsed
                .file_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&ext_lower))
            {
                continue;
            }
        }

        // Screenshot query filter
        if parsed.is_screenshot_query {
            let is_image = matches!(
                c.ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp"
            ) || c.kind == "image"
                || c.kind == "screenshot";
            let name_lower = c.name.to_lowercase();
            let path_lower = c.path.to_lowercase();
            let is_screenshot_named = is_screenshot_db
                || name_lower.contains("screenshot")
                || name_lower.contains("screen shot")
                || name_lower.contains("screen_shot")
                || name_lower.contains("capture")
                || path_lower.contains("screenshot")
                || path_lower.contains("screen shot")
                || path_lower.contains("screenshots");
            if !is_image && !is_screenshot_named {
                continue;
            }
        }

        // Date filtering
        if parsed.after.is_some() || parsed.before.is_some() {
            let time_str = if parsed.use_ctime && !c.ctime.is_empty() {
                &c.ctime
            } else {
                &c.mtime
            };

            if let Some(file_dt) = parse_db_datetime(time_str) {
                if let Some(after_dt) = parsed.after
                    && file_dt < after_dt
                {
                    continue;
                }
                if let Some(before_dt) = parsed.before
                    && file_dt > before_dt
                {
                    continue;
                }
            }
        }

        let depth_mult = depth_boost(&c.path, config.depth_weight);
        let time_for_recency = if parsed.use_ctime && !c.ctime.is_empty() {
            &c.ctime
        } else {
            &c.mtime
        };
        let recency_mult = recency_boost(
            time_for_recency,
            config.recency_weight,
            config.recency_half_life_days,
        );
        let loc_mult = location_boost(
            &c.path,
            &parsed.location_hints,
            config.location_boost_weight,
        );

        let score = 1.0 * depth_mult * recency_mult * loc_mult;

        let is_image_ext = matches!(
            c.ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
        ) || c.kind == "image"
            || c.kind == "screenshot";

        let thumbnail_path = if is_image_ext {
            let mut hasher = Sha256::new();
            hasher.update(c.path.as_bytes());
            let key = format!("{:x}", hasher.finalize());
            if let Ok(app_data_dir) = smc_core::config::AppConfig::data_dir() {
                let thumb_file = app_data_dir.join("thumbnails").join(format!("{}.jpg", key));
                if thumb_file.exists() {
                    Some(thumb_file.to_string_lossy().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let is_screenshot = is_screenshot_db
            || c.name.to_lowercase().contains("screenshot")
            || c.path.to_lowercase().contains("screenshot");

        results.push(SearchResult {
            id: c.id,
            path: c.path,
            name: c.name,
            parent_dir: c.parent_dir,
            ext: c.ext,
            size: c.size,
            mtime: c.mtime,
            kind: c.kind,
            score,
            match_type: "attribute".to_string(),
            snippet: None,
            page: None,
            section: None,
            symbol: None,
            matches: Vec::new(),
            tag: c.tag,
            masked_payload: c.masked_payload,
            thumbnail_path,
            is_screenshot,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if results.len() > limit {
        results.truncate(limit);
    }

    Ok(results)
}

struct VectorHitRaw {
    file_id: i64,
    chunk_id: i64,
    path: String,
    name: String,
    parent_dir: String,
    ext: String,
    size: i64,
    mtime: String,
    ctime: String,
    kind: String,
    score: f32,
    text: String,
    snippet: String,
    page: Option<usize>,
    section: Option<String>,
    symbol: Option<String>,
}

type ChunkRow = (String, Option<i64>, Option<String>, Option<String>);
type FileRow = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
);

fn retrieve_vector_hits(
    db: &Database,
    embedder: &dyn Embedder,
    vector_index: &dyn VectorIndex,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<VectorHitRaw>> {
    let query_vec = match embedder.embed_query(query) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "failed to embed query for hybrid search");
            return Ok(Vec::new());
        }
    };

    let matches = vector_index
        .search(&query_vec, limit, Some(embedder.model_id()))
        .map_err(|e| {
            smc_core::error::CoreError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;

    if matches.is_empty() {
        return Ok(Vec::new());
    }

    let reader = db.reader()?;
    let mut hits = Vec::with_capacity(matches.len());

    for m in matches {
        let chunk_row: Option<ChunkRow> = reader
            .query_row(
                "SELECT text, page, section, symbol FROM chunks WHERE id = ?1",
                rusqlite::params![m.chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        let file_row: Option<FileRow> = reader
            .query_row(
                "SELECT path, parent_dir, name, ext, size, mtime, ctime, kind, status FROM files WHERE id = ?1",
                rusqlite::params![m.file_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .optional()?;

        if let (
            Some((text, page, section, symbol)),
            Some((path, parent_dir, name, ext, size, mtime, ctime, kind, status)),
        ) = (chunk_row, file_row)
        {
            if status == "deleted" {
                continue;
            }

            let snippet = if text.len() <= 160 {
                text.clone()
            } else {
                format!("{}...", text[..160].trim_end())
            };

            hits.push(VectorHitRaw {
                file_id: m.file_id,
                chunk_id: m.chunk_id,
                path,
                name,
                parent_dir,
                ext,
                size,
                mtime,
                ctime,
                kind,
                score: m.score,
                text,
                snippet,
                page: page.map(|p| p as usize),
                section,
                symbol,
            });
        }
    }

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smc_core::db::Database;
    use smc_embed::error::EmbedResult;
    use smc_embed::vector_index::VectorRecord;

    fn setup_hybrid_test_db() -> Database {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("hybrid_test.db");
        let db = Database::open(&db_path).unwrap();
        std::mem::forget(tmp);

        {
            let conn = db.writer();

            // 1. File that matches by name only
            conn.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (1, '/notes/architecture.txt', '/notes', 'architecture.txt', 'txt', 100, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 'text', 'active', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();

            // 2. File that matches by content only
            conn.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (2, '/notes/meeting.txt', '/notes', 'meeting.txt', 'txt', 200, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 'text', 'active', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (1, 2, 0, 'Discussed neural indexing and vector embeddings in the meeting.', 1, NULL, NULL, 0, 70)",
                [],
            ).unwrap();

            // 3. File that matches by both filename and content (Hybrid)
            conn.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (3, '/notes/embeddings_overview.txt', '/notes', 'embeddings_overview.txt', 'txt', 300, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 'text', 'active', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (2, 3, 0, 'Comprehensive overview of neural embeddings and vector similarity.', 1, NULL, NULL, 0, 68)",
                [],
            ).unwrap();

            // 4. File that matches via Image Tags (QR Code)
            conn.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (4, '/images/qr_payment.png', '/images', 'qr_payment.png', 'png', 500, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', 'image', 'active', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO image_tags (id, file_id, tag, payload, created_at)
                 VALUES (1, 4, 'qr:payment', 'upi://pay?pa=acc***@bank', '2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO image_metadata (file_id, width, height, format, exif_date, camera_make, camera_model, is_screenshot, has_qr, qr_count)
                 VALUES (4, 1920, 1080, 'png', '2024-01-01T00:00:00Z', NULL, NULL, 1, 1, 1)",
                [],
            ).unwrap();
        }

        db
    }

    struct MockEmbedder;
    impl Embedder for MockEmbedder {
        fn embed_documents(&self, docs: &[&str]) -> EmbedResult<Vec<Vec<f32>>> {
            Ok(vec![vec![1.0, 0.0, 0.0]; docs.len()])
        }
        fn embed_query(&self, _query: &str) -> EmbedResult<Vec<f32>> {
            Ok(vec![1.0, 0.0, 0.0])
        }
        fn dims(&self) -> usize {
            3
        }
        fn model_id(&self) -> &str {
            "mock-model"
        }
        fn unload(&self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn maybe_unload_idle(&self) -> bool {
            false
        }
    }

    struct MockVectorIndex;
    impl VectorIndex for MockVectorIndex {
        fn insert(&self, _record: &VectorRecord) -> smc_embed::error::EmbedResult<()> {
            Ok(())
        }
        fn insert_batch(&self, _records: &[VectorRecord]) -> smc_embed::error::EmbedResult<()> {
            Ok(())
        }
        fn search(
            &self,
            _query: &[f32],
            _limit: usize,
            _model_id: Option<&str>,
        ) -> smc_embed::error::EmbedResult<Vec<smc_embed::vector_index::VectorMatch>> {
            Ok(vec![smc_embed::vector_index::VectorMatch {
                chunk_id: 2,
                file_id: 3,
                score: 0.95,
            }])
        }
        fn delete_for_file(&self, _file_id: i64) -> smc_embed::error::EmbedResult<usize> {
            Ok(1)
        }
        fn delete_for_chunk(&self, _chunk_id: i64) -> smc_embed::error::EmbedResult<usize> {
            Ok(1)
        }
        fn get_indexed_chunk_hashes(
            &self,
            _file_id: i64,
            _model_id: &str,
        ) -> smc_embed::error::EmbedResult<HashMap<i64, String>> {
            Ok(HashMap::new())
        }
        fn count(&self, _model_id: Option<&str>) -> smc_embed::error::EmbedResult<usize> {
            Ok(1)
        }
        fn clear(&self) -> smc_embed::error::EmbedResult<()> {
            Ok(())
        }
        fn get_vector(&self, _chunk_id: i64) -> smc_embed::error::EmbedResult<Option<Vec<f32>>> {
            Ok(None)
        }
    }

    #[test]
    fn test_hybrid_search_scoring_and_ranking() {
        let db = setup_hybrid_test_db();
        let embedder = MockEmbedder;
        let vector_index = MockVectorIndex;

        let results = hybrid_search_nlq_full(
            &db,
            Some(&embedder),
            Some(&vector_index),
            "embeddings",
            10,
            &RankingConfig::default(),
        )
        .unwrap();

        assert!(!results.is_empty());
        // File 3 matches filename, content, and vector -> should rank #1
        assert_eq!(results[0].id, 3);
        assert_eq!(results[0].match_type, "hybrid");
    }

    #[test]
    fn test_image_tag_search_and_classification() {
        let db = setup_hybrid_test_db();
        let parsed = smc_nlq::parse_query("screenshots with a payment QR");

        let results = hybrid_search_with_parsed_query(
            &db,
            None,
            None,
            &parsed,
            10,
            &RankingConfig::default(),
        )
        .unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].id, 4);
        assert_eq!(results[0].tag.as_deref(), Some("qr:payment"));
        assert!(results[0].is_screenshot);
    }
}
