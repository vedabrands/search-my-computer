use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Unified configuration holding all tunable ranking parameters, weights, and priors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankingConfig {
    /// RRF smoothing constant (standard: 60.0).
    pub rrf_k: f64,
    /// RRF weight multiplier for filename stream.
    pub weight_filename: f64,
    /// RRF weight multiplier for content BM25 stream.
    pub weight_content: f64,
    /// RRF weight multiplier for vector semantic stream.
    pub weight_vector: f64,
    /// Boost for exact filename match (added to 1.0 base).
    pub exact_name_boost: f64,
    /// Boost for exact filename stem match (added to 1.0 base).
    pub stem_name_boost: f64,
    /// Boost for prefix filename match (added to 1.0 base).
    pub prefix_name_boost: f64,
    /// Recency boost maximum weight.
    pub recency_weight: f64,
    /// Recency half-life in days for exponential decay.
    pub recency_half_life_days: f64,
    /// Depth prior weight factor.
    pub depth_weight: f64,
    /// Reinforcement bonus per additional distinct matching chunk.
    pub multi_chunk_bonus: f64,
    /// Jaccard similarity threshold for near-duplicate chunk suppression.
    pub chunk_similarity_threshold: f64,
    /// Maximum additional chunk matches to retain per file.
    pub max_additional_matches: usize,
    /// File type intent prior weight.
    pub file_type_prior_weight: f64,
    /// Location hint boost weight.
    pub location_boost_weight: f64,
    /// Image tag match weight for RRF.
    pub weight_image_tag: f64,
    /// Image visual vector match weight for RRF.
    pub weight_image_vector: f64,
    /// Image tag boost multiplier weight.
    pub tag_boost_weight: f64,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            rrf_k: 60.0,
            weight_filename: 1.2,
            weight_content: 1.0,
            weight_vector: 1.1,
            exact_name_boost: 5.0,
            stem_name_boost: 3.0,
            prefix_name_boost: 1.5,
            recency_weight: 0.3,
            recency_half_life_days: 90.0,
            depth_weight: 0.4,
            multi_chunk_bonus: 0.05,
            chunk_similarity_threshold: 0.6,
            max_additional_matches: 3,
            file_type_prior_weight: 0.3,
            location_boost_weight: 0.8,
            weight_image_tag: 1.2,
            weight_image_vector: 1.1,
            tag_boost_weight: 1.0,
        }
    }
}

/// Calculate Reciprocal Rank Fusion component for a 1-based rank.
/// Formula: weight / (k + rank)
pub fn rrf_score(rank: usize, k: f64, weight: f64) -> f64 {
    if rank == 0 {
        return 0.0;
    }
    weight / (k + rank as f64)
}

/// Calculate a depth boost factor. Shallower paths score higher.
/// Multiplier = 1.0 + (depth_weight / (1.0 + depth))
pub fn depth_boost(path: &str, depth_weight: f64) -> f64 {
    let normalized = path.replace('\\', "/");
    let depth = normalized.matches('/').count() as f64;
    1.0 + depth_weight / (1.0 + depth)
}

/// Calculate recency boost factor using exponential half-life decay.
/// Multiplier = 1.0 + recency_weight * 2^(-age_days / half_life_days)
pub fn recency_boost(mtime: &str, recency_weight: f64, half_life_days: f64) -> f64 {
    let parsed = DateTime::parse_from_rfc3339(mtime)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let age_days = (Utc::now() - parsed).num_days().max(0) as f64;
    let decay = (-age_days / half_life_days.max(1.0)).exp2();
    1.0 + recency_weight * decay
}

/// Match-type boost multiplier based on query and filename.
pub fn match_type_boost(name: &str, query: &str, config: &RankingConfig) -> f64 {
    let name_lower = name.to_lowercase();
    let query_lower = query.to_lowercase().trim().to_string();

    if query_lower.is_empty() {
        return 1.0;
    }

    if name_lower == query_lower {
        // Exact full filename match
        1.0 + config.exact_name_boost
    } else if let Some(stem) = name_lower.rsplit_once('.').map(|(s, _)| s) {
        if stem == query_lower {
            // Exact stem match (e.g. "report" matching "report.docx")
            1.0 + config.stem_name_boost
        } else if stem.starts_with(&query_lower) {
            // Prefix stem match
            1.0 + config.prefix_name_boost
        } else {
            1.0
        }
    } else if name_lower.starts_with(&query_lower) {
        1.0 + config.prefix_name_boost
    } else {
        1.0
    }
}

/// Location hint boost factor. Matches if any location hint appears in the file path.
/// Multiplier = 1.0 + location_boost_weight if path contains any location hint, else 1.0.
pub fn location_boost(path: &str, hints: &[String], location_boost_weight: f64) -> f64 {
    if hints.is_empty() {
        return 1.0;
    }
    let path_lower = path.to_lowercase().replace('\\', "/");
    for hint in hints {
        let hint_lower = hint.to_lowercase().trim().to_string();
        if !hint_lower.is_empty() && path_lower.contains(&hint_lower) {
            return 1.0 + location_boost_weight;
        }
    }
    1.0
}

/// Tag filter match boost factor.
/// Multiplier = 1.0 + tag_boost_weight if file matches tag filter, else 1.0.
pub fn tag_boost(has_tag_match: bool, tag_boost_weight: f64) -> f64 {
    if has_tag_match {
        1.0 + tag_boost_weight
    } else {
        1.0
    }
}

/// Query intent classification for file-type prior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    Code,
    Document,
    Image,
    General,
}

/// Classifies query intent to determine file-type affinity.
pub fn classify_query_intent(query: &str) -> QueryIntent {
    let q = query.trim();
    if q.is_empty() {
        return QueryIntent::General;
    }

    // Image indicators: screenshot, photo, qr, picture, whiteboard, etc.
    let q_lower = q.to_lowercase();
    let has_image_indicators = [
        "screenshot",
        "screen shot",
        "screengrab",
        "photo",
        "picture",
        "qr",
        "barcode",
        "whiteboard",
        "wallpaper",
        "drawing",
    ]
    .iter()
    .any(|kw| q_lower.contains(kw));

    if has_image_indicators {
        return QueryIntent::Image;
    }

    // Code indicators: symbols, camelCase, snake_case with underscores, code keywords
    let has_code_keywords = [
        "fn ",
        "def ",
        "class ",
        "import ",
        "async ",
        "const ",
        "struct ",
        "impl ",
        "interface ",
        "export ",
        "public ",
        "private ",
        "void ",
        "return ",
        "typedef ",
    ]
    .iter()
    .any(|kw| q.starts_with(kw) || q.contains(&format!(" {}", kw)));

    let has_code_syntax = q.contains("::")
        || q.contains("->")
        || q.contains("()")
        || q.contains("=>")
        || q.contains("==")
        || q.contains("!=")
        || (q.contains('_') && !q.contains(' '))
        || (q.chars().any(|c| c.is_uppercase())
            && q.chars().any(|c| c.is_lowercase())
            && !q.contains(' '));

    if has_code_keywords || has_code_syntax {
        return QueryIntent::Code;
    }

    // Document indicators: natural language question words or document terms
    let has_doc_indicators = [
        "pdf",
        "report",
        "presentation",
        "sheet",
        "summary",
        "notes",
        "slides",
        "proposal",
        "invoice",
        "manual",
        "guide",
        "whitepaper",
        "budget",
        "minutes",
        "downloaded",
    ]
    .iter()
    .any(|kw| q.to_lowercase().contains(kw));

    let word_count = q.split_whitespace().count();
    if has_doc_indicators || (word_count >= 4 && !q.contains('{') && !q.contains('(')) {
        return QueryIntent::Document;
    }

    QueryIntent::General
}

/// Computes file-type prior multiplier given query intent and file extension / kind.
pub fn file_type_prior(intent: QueryIntent, ext: &str, kind: &str, prior_weight: f64) -> f64 {
    let ext_lower = ext.to_lowercase();
    let is_code = matches!(
        ext_lower.as_str(),
        "rs" | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "go"
            | "java"
            | "cs"
            | "rb"
            | "php"
            | "swift"
            | "kt"
    ) || kind == "code";

    let is_doc = matches!(
        ext_lower.as_str(),
        "pdf" | "docx" | "doc" | "pptx" | "ppt" | "xlsx" | "xls" | "md" | "txt" | "csv" | "rtf"
    ) || kind == "document"
        || kind == "pdf"
        || kind == "text";

    let is_image = matches!(
        ext_lower.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "tiff" | "ico"
    ) || kind == "image"
        || kind == "screenshot";

    match intent {
        QueryIntent::Code => {
            if is_code {
                1.0 + prior_weight
            } else if is_doc || is_image {
                1.0 / (1.0 + prior_weight)
            } else {
                1.0
            }
        }
        QueryIntent::Document => {
            if is_doc {
                1.0 + prior_weight
            } else if is_code || is_image {
                1.0 / (1.0 + prior_weight)
            } else {
                1.0
            }
        }
        QueryIntent::Image => {
            if is_image {
                1.0 + prior_weight
            } else if is_code {
                1.0 / (1.0 + prior_weight)
            } else {
                1.0
            }
        }
        QueryIntent::General => 1.0,
    }
}

/// Computes character 3-gram Jaccard similarity between two text snippets.
pub fn character_trigram_jaccard(a: &str, b: &str) -> f64 {
    let ngrams_a = extract_trigrams(a);
    let ngrams_b = extract_trigrams(b);

    if ngrams_a.is_empty() || ngrams_b.is_empty() {
        return 0.0;
    }

    let intersection_count = ngrams_a.intersection(&ngrams_b).count();
    let union_count = ngrams_a.union(&ngrams_b).count();

    if union_count == 0 {
        0.0
    } else {
        intersection_count as f64 / union_count as f64
    }
}

fn extract_trigrams(text: &str) -> HashSet<String> {
    let clean: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();

    let chars: Vec<char> = clean.chars().collect();
    if chars.len() < 3 {
        return HashSet::new();
    }

    let mut set = HashSet::with_capacity(chars.len() - 2);
    for window in chars.windows(3) {
        set.insert(window.iter().collect::<String>());
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_score() {
        let r1 = rrf_score(1, 60.0, 1.0);
        let r2 = rrf_score(2, 60.0, 1.0);
        let r0 = rrf_score(0, 60.0, 1.0);

        assert_eq!(r0, 0.0);
        assert!((r1 - 1.0 / 61.0).abs() < 1e-9);
        assert!((r2 - 1.0 / 62.0).abs() < 1e-9);
        assert!(r1 > r2);
    }

    #[test]
    fn test_depth_boost_shallow_wins() {
        let shallow = depth_boost("C:/Users/docs/file.txt", 0.4);
        let deep = depth_boost("C:/Users/docs/a/b/c/d/e/file.txt", 0.4);
        assert!(shallow > deep);
        assert!(shallow > 1.0);
    }

    #[test]
    fn test_recency_boost_exponential_decay() {
        let now = Utc::now().to_rfc3339();
        let old = "2020-01-01T00:00:00+00:00";

        let recent = recency_boost(&now, 0.3, 90.0);
        let ancient = recency_boost(old, 0.3, 90.0);

        assert!(recent > ancient);
        assert!((recent - 1.3).abs() < 0.05); // immediate gives ~1.3
        assert!((ancient - 1.0).abs() < 0.01); // ancient decays to ~1.0
    }

    #[test]
    fn test_match_type_exact() {
        let cfg = RankingConfig::default();
        assert_eq!(match_type_boost("README.md", "README.md", &cfg), 6.0);
        assert_eq!(match_type_boost("README.md", "README", &cfg), 4.0);
        assert_eq!(match_type_boost("README.md", "READ", &cfg), 2.5);
        assert_eq!(match_type_boost("README.md", "EADM", &cfg), 1.0);
    }

    #[test]
    fn test_query_intent_classification() {
        assert_eq!(
            classify_query_intent("fn calculate_hash"),
            QueryIntent::Code
        );
        assert_eq!(classify_query_intent("getUserById"), QueryIntent::Code);
        assert_eq!(classify_query_intent("get_user_id"), QueryIntent::Code);
        assert_eq!(
            classify_query_intent("financial quarterly report pdf"),
            QueryIntent::Document
        );
        assert_eq!(
            classify_query_intent("meeting notes from team sprint"),
            QueryIntent::Document
        );
        assert_eq!(classify_query_intent("project"), QueryIntent::General);
    }

    #[test]
    fn test_file_type_prior() {
        let doc_boost = file_type_prior(QueryIntent::Document, "pdf", "pdf", 0.3);
        let doc_penalty = file_type_prior(QueryIntent::Document, "rs", "code", 0.3);
        assert!(doc_boost > 1.0);
        assert!(doc_penalty < 1.0);

        let code_boost = file_type_prior(QueryIntent::Code, "rs", "code", 0.3);
        let code_penalty = file_type_prior(QueryIntent::Code, "docx", "document", 0.3);
        assert!(code_boost > 1.0);
        assert!(code_penalty < 1.0);
    }

    #[test]
    fn test_trigram_jaccard_similarity() {
        let text1 = "The quarterly revenue report shows a 25 percent increase.";
        let text2 = "The quarterly revenue report shows a 25 percent increase in sales.";
        let text3 = "Machine learning models and neural network embeddings.";

        let sim_high = character_trigram_jaccard(text1, text2);
        let sim_low = character_trigram_jaccard(text1, text3);

        assert!(sim_high > 0.7);
        assert!(sim_low < 0.2);
    }

    #[test]
    fn test_location_boost() {
        let hints = vec!["downloads".to_string(), "desktop".to_string()];
        let path1 = "C:/Users/dev/Downloads/report.pdf";
        let path2 = "C:/Users/dev/Documents/report.pdf";
        let boost1 = location_boost(path1, &hints, 0.8);
        let boost2 = location_boost(path2, &hints, 0.8);
        assert_eq!(boost1, 1.8);
        assert_eq!(boost2, 1.0);
    }
}
