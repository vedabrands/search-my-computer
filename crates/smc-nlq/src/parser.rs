//! Deterministic rule-based Natural Language Query parser.
//!
//! Extracts date ranges, file types, location hints, and intent signals from natural language queries
//! without external ML dependencies or cloud services.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// Parsed filter criteria and clean text from a natural language query.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ParsedQuery {
    /// Raw original query.
    pub raw: String,
    /// Cleaned search terms after removing filler and filter tokens (for vector/BM25 search).
    pub text: String,
    /// Extracted file extensions (e.g. `["pdf"]`, `["docx", "doc"]`).
    pub file_types: Vec<String>,
    /// Optional date lower bound (UTC).
    pub after: Option<DateTime<Utc>>,
    /// Optional date upper bound (UTC).
    pub before: Option<DateTime<Utc>>,
    /// Location hints (e.g. `["downloads"]`, `["desktop"]`, `["documents"]`, folder names).
    pub location_hints: Vec<String>,
    /// Structured semantic tag filters (e.g. `["qr:payment"]`, `["type:screenshot"]`, `["qr:*"]`).
    pub tag_filters: Vec<String>,
    /// Whether query targets a project/repository entity.
    pub is_project_query: bool,
    /// Whether query specifically asks for screenshots.
    pub is_screenshot_query: bool,
    /// Whether query targets visual semantic concepts (e.g. "photo of a whiteboard", "receipt").
    pub is_visual_query: bool,
    /// Whether to prioritize file creation time (ctime) over modification time (mtime).
    pub use_ctime: bool,
}

impl ParsedQuery {
    /// Create a basic query with no filters.
    pub fn plain(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let text = raw.trim().to_string();
        Self {
            raw,
            text,
            file_types: Vec::new(),
            after: None,
            before: None,
            location_hints: Vec::new(),
            tag_filters: Vec::new(),
            is_project_query: false,
            is_screenshot_query: false,
            is_visual_query: false,
            use_ctime: false,
        }
    }

    /// Returns true if any filter (type, date, location, or intent) is active.
    pub fn has_filters(&self) -> bool {
        !self.file_types.is_empty()
            || self.after.is_some()
            || self.before.is_some()
            || !self.location_hints.is_empty()
            || !self.tag_filters.is_empty()
            || self.is_project_query
            || self.is_screenshot_query
            || self.is_visual_query
    }
}

/// Helper to get the start of a UTC day (00:00:00).
fn start_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .single()
        .unwrap_or(dt)
}

/// Helper to get the end of a UTC day (23:59:59).
fn end_of_day(dt: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 23, 59, 59)
        .single()
        .unwrap_or(dt)
}

/// Helper to get the number of days in a month.
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Parse a natural language query using the current system time in UTC.
pub fn parse_query(raw: &str) -> ParsedQuery {
    parse_query_with_ref_time(raw, Utc::now())
}

/// Parse a natural language query with a deterministic reference timestamp.
pub fn parse_query_with_ref_time(raw: &str, now: DateTime<Utc>) -> ParsedQuery {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ParsedQuery::default();
    }

    let mut after: Option<DateTime<Utc>> = None;
    let mut before: Option<DateTime<Utc>> = None;
    let mut file_types: Vec<String> = Vec::new();
    let mut location_hints: Vec<String> = Vec::new();
    let mut tag_filters: Vec<String> = Vec::new();
    let mut is_project_query = false;
    let mut is_screenshot_query = false;
    let mut is_visual_query = false;
    let mut use_ctime = false;

    let lower = trimmed.to_lowercase();
    let mut matched_spans: Vec<(usize, usize)> = Vec::new();

    // Helper closure to check if a span overlaps existing matched spans
    let overlaps = |start: usize, end: usize, spans: &[(usize, usize)]| -> bool {
        spans.iter().any(|&(s, e)| start < e && end > s)
    };

    // 1. Check for intent phrases
    // "where is my <name> project", "where is <name> repo", "project", "repo", "repository"
    for kw in &[
        "project",
        "projects",
        "repo",
        "repos",
        "repository",
        "repositories",
        "workspace",
        "workspaces",
    ] {
        if find_word(&lower, kw).is_some() {
            is_project_query = true;
            break;
        }
    }

    let has_conceptual_terms = [
        "plan",
        "plans",
        "roadmap",
        "vision",
        "future",
        "prd",
        "rfc",
        "spec",
        "whitepaper",
        "guide",
        "design",
        "architecture",
        "notes",
        "meeting",
        "discussion",
        "document",
        "documents",
        "doc",
        "docs",
        "presentation",
        "slides",
        "ocr",
    ]
    .iter()
    .any(|w| find_word(&lower, w).is_some());

    if !has_conceptual_terms {
        for kw in &["screen capture", "screenshots", "screenshot", "snip"] {
            if let Some(pos) = find_word(&lower, kw) {
                is_screenshot_query = true;
                if !tag_filters.contains(&"type:screenshot".to_string()) {
                    tag_filters.push("type:screenshot".to_string());
                }
                matched_spans.push((pos, pos + kw.len()));
                for ext in &["png", "jpg", "jpeg", "webp"] {
                    if !file_types.contains(&ext.to_string()) {
                        file_types.push(ext.to_string());
                    }
                }
                break;
            }
        }
    }

    // QR & Barcode tag patterns
    let qr_mappings: &[(&[&str], &str)] = &[
        (
            &[
                "payment qr code",
                "payment qr codes",
                "payment qr",
                "upi qr code",
                "upi qr codes",
                "upi qr",
                "upi payment qr",
                "crypto qr code",
                "crypto qr",
                "bitcoin qr",
                "payment barcode",
            ],
            "qr:payment",
        ),
        (
            &[
                "wifi qr code",
                "wifi qr codes",
                "wifi qr",
                "wi-fi qr code",
                "wi-fi qr codes",
                "wi-fi qr",
                "wifi barcode",
            ],
            "qr:wifi",
        ),
        (
            &[
                "contact qr code",
                "contact qr codes",
                "contact qr",
                "vcard qr code",
                "vcard qr",
                "business card qr",
            ],
            "qr:contact",
        ),
        (
            &[
                "url qr code",
                "url qr codes",
                "url qr",
                "link qr code",
                "link qr",
                "website qr code",
                "website qr",
            ],
            "qr:url",
        ),
        (
            &[
                "qr code",
                "qr codes",
                "qr",
                "barcode",
                "barcodes",
                "2d barcode",
            ],
            "qr:*",
        ),
    ];

    for &(keywords, tag) in qr_mappings {
        let mut matched = false;
        for &kw in keywords {
            if let Some(pos) = find_word(&lower, kw) {
                if !tag_filters.contains(&tag.to_string()) {
                    tag_filters.push(tag.to_string());
                }
                is_visual_query = true;
                matched_spans.push((pos, pos + kw.len()));
                matched = true;
                for ext in &["png", "jpg", "jpeg", "webp"] {
                    if !file_types.contains(&ext.to_string()) {
                        file_types.push(ext.to_string());
                    }
                }
                break;
            }
        }
        if matched {
            break;
        }
    }

    // Photo & visual keywords
    for kw in &[
        "photo of",
        "photos of",
        "picture of",
        "pictures of",
        "image of",
        "images of",
        "photograph of",
        "photographs of",
        "photo",
        "photos",
        "picture",
        "pictures",
        "photograph",
        "photographs",
    ] {
        if let Some(pos) = find_word(&lower, kw) {
            is_visual_query = true;
            if !tag_filters.contains(&"type:photo".to_string()) {
                tag_filters.push("type:photo".to_string());
            }
            matched_spans.push((pos, pos + kw.len()));
            for ext in &["png", "jpg", "jpeg", "webp"] {
                if !file_types.contains(&ext.to_string()) {
                    file_types.push(ext.to_string());
                }
            }
            break;
        }
    }

    // Receipt / Invoice terms (triggers visual query for OCR/CLIP matching without phantom tag)
    for kw in &["receipt", "receipts", "invoice", "invoices"] {
        if find_word(&lower, kw).is_some() {
            is_visual_query = true;
            break;
        }
    }

    // Visual concepts
    for kw in &[
        "whiteboard",
        "whiteboards",
        "sketch",
        "drawing",
        "diagram",
        "sunset",
        "mountain",
        "beach",
    ] {
        if find_word(&lower, kw).is_some() {
            is_visual_query = true;
            break;
        }
    }

    // 2. Location phrases
    if let Some(pos) = find_word(&lower, "downloaded") {
        use_ctime = true;
        if !location_hints.contains(&"downloads".to_string()) {
            location_hints.push("downloads".to_string());
        }
        matched_spans.push((pos, pos + 10));
    }
    if let Some(pos) = find_word(&lower, "downloads") {
        if !location_hints.contains(&"downloads".to_string()) {
            location_hints.push("downloads".to_string());
        }
        matched_spans.push((pos, pos + 9));
    }
    if lower.contains("download folder") {
        if !location_hints.contains(&"downloads".to_string()) {
            location_hints.push("downloads".to_string());
        }
        if let Some(pos) = lower.find("download folder") {
            matched_spans.push((pos, pos + 15));
        }
    }
    if let Some(pos) = find_word(&lower, "desktop") {
        if !location_hints.contains(&"desktop".to_string()) {
            location_hints.push("desktop".to_string());
        }
        matched_spans.push((pos, pos + 7));
    }
    if let Some(pos) = find_word(&lower, "documents") {
        if !location_hints.contains(&"documents".to_string()) {
            location_hints.push("documents".to_string());
        }
        matched_spans.push((pos, pos + 9));
    }

    // Pattern: "under <folder>"
    let mut search_idx = 0;
    while let Some(pos) = lower[search_idx..].find("under ") {
        let abs_pos = search_idx + pos;
        let prev_ok = abs_pos == 0 || !lower.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
        if prev_ok {
            let rem = &lower[abs_pos + 6..];
            let token = rem.split_whitespace().next().unwrap_or("");
            let clean = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !clean.is_empty() && clean != "the" && clean != "a" && clean != "an" && clean != "my"
            {
                if !location_hints.contains(&clean.to_string()) {
                    location_hints.push(clean.to_string());
                }
                matched_spans.push((abs_pos, abs_pos + 6 + token.len()));
            }
        }
        search_idx = abs_pos + 6;
    }

    // Pattern: "in <folder> folder", "in <folder> directory", "in folder <folder>", "in directory <folder>",
    // and "in <folder>" (where folder is not date, type, or stopword)
    let mut search_idx = 0;
    while let Some(pos) = lower[search_idx..].find("in ") {
        let abs_pos = search_idx + pos;
        let prev_ok = abs_pos == 0 || !lower.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
        if prev_ok {
            let rem = &lower[abs_pos + 3..];
            let mut words = rem.split_whitespace();
            if let Some(w1) = words.next() {
                let clean1 =
                    w1.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                let w2 = words.next().unwrap_or("");
                let clean2 =
                    w2.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');

                if clean1 == "folder" || clean1 == "directory" {
                    if !clean2.is_empty()
                        && clean2 != "the"
                        && clean2 != "my"
                        && clean2 != "a"
                        && clean2 != "an"
                    {
                        if !location_hints.contains(&clean2.to_string()) {
                            location_hints.push(clean2.to_string());
                        }
                        matched_spans.push((abs_pos, abs_pos + 3 + w1.len() + 1 + w2.len()));
                    }
                } else if clean2 == "folder" || clean2 == "directory" {
                    if !clean1.is_empty()
                        && clean1 != "the"
                        && clean1 != "my"
                        && clean1 != "a"
                        && clean1 != "an"
                    {
                        if !location_hints.contains(&clean1.to_string()) {
                            location_hints.push(clean1.to_string());
                        }
                        matched_spans.push((abs_pos, abs_pos + 3 + w1.len() + 1 + w2.len()));
                    }
                } else {
                    let is_month = matches!(
                        clean1,
                        "january"
                            | "jan"
                            | "february"
                            | "feb"
                            | "march"
                            | "mar"
                            | "april"
                            | "apr"
                            | "may"
                            | "june"
                            | "jun"
                            | "july"
                            | "jul"
                            | "august"
                            | "aug"
                            | "september"
                            | "sep"
                            | "sept"
                            | "october"
                            | "oct"
                            | "november"
                            | "nov"
                            | "december"
                            | "dec"
                    );
                    let is_year = clean1.len() == 4 && clean1.chars().all(|c| c.is_ascii_digit());
                    let is_type = matches!(
                        clean1,
                        "rust"
                            | "rs"
                            | "python"
                            | "py"
                            | "typescript"
                            | "ts"
                            | "javascript"
                            | "js"
                            | "golang"
                            | "go"
                            | "cpp"
                            | "c"
                            | "java"
                            | "sql"
                            | "pdf"
                            | "word"
                            | "excel"
                            | "ppt"
                            | "markdown"
                            | "zip"
                            | "tar"
                    );
                    let is_stop = matches!(
                        clean1,
                        "the"
                            | "a"
                            | "an"
                            | "my"
                            | "this"
                            | "that"
                            | "all"
                            | "our"
                            | "their"
                            | "detail"
                            | "full"
                            | "english"
                            | "total"
                            | "brief"
                            | "general"
                            | "high"
                            | "low"
                            | "today"
                            | "yesterday"
                            | "production"
                            | "prod"
                            | "staging"
                            | "dev"
                            | "development"
                            | "test"
                            | "testing"
                            | "release"
                            | "memory"
                            | "parallel"
                            | "background"
                            | "cloud"
                            | "docker"
                            | "kubernetes"
                            | "practice"
                            | "theory"
                            | "realtime"
                            | "action"
                            | "terms"
                            | "case"
                            | "cases"
                            | "order"
                            | "place"
                            | "addition"
                            | "part"
                            | "particular"
                            | "advance"
                            | "summary"
                            | "fact"
                            | "effect"
                            | "use"
                            | "response"
                            | "return"
                            | "context"
                            | "progress"
                            | "question"
                            | "mind"
                            | "view"
                            | "first"
                            | "second"
                            | "third"
                            | "fourth"
                            | "quarter"
                            | "q1"
                            | "q2"
                            | "q3"
                            | "q4"
                    );

                    if !is_month && !is_year && !is_type && !is_stop && clean1.len() >= 2 {
                        if !location_hints.contains(&clean1.to_string()) {
                            location_hints.push(clean1.to_string());
                        }
                        matched_spans.push((abs_pos, abs_pos + 3 + w1.len()));
                    }
                }
            }
        }
        search_idx = abs_pos + 3;
    }

    // 3. File type lexicon detection
    let type_mappings: &[(&[&str], &[&str])] = &[
        (&["pdf", "pdfs"], &["pdf"]),
        (
            &[
                "docx",
                "doc",
                "word doc",
                "word docs",
                "word document",
                "word documents",
                "word",
            ],
            &["docx", "doc", "rtf", "odt"],
        ),
        (
            &[
                "spreadsheet",
                "spreadsheets",
                "excel",
                "excel sheet",
                "excel sheets",
                "xlsx",
                "xls",
                "csv",
            ],
            &["xlsx", "xls", "csv", "ods"],
        ),
        (
            &[
                "slide",
                "slides",
                "presentation",
                "presentations",
                "powerpoint",
                "ppt",
                "pptx",
            ],
            &["pptx", "ppt", "odp"],
        ),
        (
            &[
                "image", "images", "photo", "photos", "picture", "pictures", "png", "jpg", "jpeg",
                "svg",
            ],
            &["png", "jpg", "jpeg", "webp", "svg", "gif"],
        ),
        (
            &[
                "source code",
                "source files",
                "source file",
                "script",
                "scripts",
                "code",
            ],
            &[
                "rs", "ts", "tsx", "js", "jsx", "py", "go", "c", "cpp", "h", "hpp", "java", "sql",
                "sh", "ps1",
            ],
        ),
        (
            &["rust", "rs file", "rs files", "rs code", "rs source"],
            &["rs"],
        ),
        (
            &[
                "python",
                "py file",
                "py files",
                "py code",
                "py source",
                "python script",
                "python scripts",
            ],
            &["py"],
        ),
        (
            &["typescript", "ts file", "ts files", "ts code", "ts source"],
            &["ts", "tsx"],
        ),
        (
            &["javascript", "js file", "js files", "js code", "js source"],
            &["js", "jsx"],
        ),
        (
            &[
                "golang",
                "go file",
                "go files",
                "go source files",
                "go source file",
                "go source",
                "go code",
            ],
            &["go"],
        ),
        (
            &[
                "cpp",
                "c++",
                "c++ header files",
                "c++ header",
                "c header files",
                "c header",
                "c file",
                "c files",
                "c++ file",
                "c++ files",
            ],
            &["cpp", "c", "h", "hpp"],
        ),
        (
            &[
                "sql",
                "sql query",
                "sql script",
                "sql scripts",
                "sql migration scripts",
            ],
            &["sql"],
        ),
        (
            &[
                "markdown",
                "md file",
                "md files",
                "note",
                "notes",
                "text file",
                "txt",
            ],
            &["md", "txt", "org"],
        ),
        (
            &[
                "archive",
                "zip",
                "tar",
                "tarball",
                "zip file",
                "zip files",
                "tar archive",
            ],
            &["zip", "tar", "gz", "7z", "rar"],
        ),
        (
            &["yaml", "yml", "yaml file", "yaml files"],
            &["yaml", "yml"],
        ),
        (&["json", "json file", "json files"], &["json"]),
        (
            &[
                "terraform",
                "tf file",
                "tf files",
                "terraform config",
                "terraform file",
            ],
            &["tf"],
        ),
    ];

    for &(keywords, extensions) in type_mappings {
        for &kw in keywords {
            // Check for word boundary around keyword in lower
            let mut search_idx = 0;
            while let Some(found_idx) = lower[search_idx..].find(kw) {
                let abs_idx = search_idx + found_idx;
                let end_idx = abs_idx + kw.len();
                search_idx = end_idx;

                let prev_ok =
                    abs_idx == 0 || !lower.as_bytes()[abs_idx - 1].is_ascii_alphanumeric();
                let next_ok =
                    end_idx == lower.len() || !lower.as_bytes()[end_idx].is_ascii_alphanumeric();

                if prev_ok && next_ok {
                    for ext in extensions {
                        if !file_types.contains(&ext.to_string()) {
                            file_types.push(ext.to_string());
                        }
                    }
                    matched_spans.push((abs_idx, end_idx));
                }
            }
        }
    }

    // 4. Temporal range parsing
    // Check multi-word date patterns first, then relative patterns, then single words

    // "today"
    if let Some(pos) = find_word(&lower, "today") {
        after = Some(start_of_day(now));
        before = Some(end_of_day(now));
        matched_spans.push((pos, pos + 5));
    }

    // "yesterday"
    if let Some(pos) = find_word(&lower, "yesterday") {
        let yday = now - Duration::days(1);
        after = Some(start_of_day(yday));
        before = Some(end_of_day(yday));
        matched_spans.push((pos, pos + 9));
    }

    // "this week"
    if let Some(pos) = lower.find("this week") {
        let days_from_mon = now.weekday().num_days_from_monday();
        let mon = now - Duration::days(days_from_mon as i64);
        after = Some(start_of_day(mon));
        before = Some(end_of_day(now));
        matched_spans.push((pos, pos + 9));
    }

    // "last week" / "past week"
    for phrase in &["last week", "past week", "previous week"] {
        if let Some(pos) = lower.find(phrase) {
            let days_from_mon = now.weekday().num_days_from_monday();
            let this_mon = now - Duration::days(days_from_mon as i64);
            let last_mon = this_mon - Duration::days(7);
            let last_sun = this_mon - Duration::days(1);
            after = Some(start_of_day(last_mon));
            before = Some(end_of_day(last_sun));
            matched_spans.push((pos, pos + phrase.len()));
            break;
        }
    }

    // "this month"
    if let Some(pos) = lower.find("this month")
        && let Some(first_day) = Utc
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .single()
    {
        after = Some(first_day);
        before = Some(end_of_day(now));
        matched_spans.push((pos, pos + 10));
    }

    // "last month" / "past month"
    for phrase in &["last month", "past month", "previous month"] {
        if let Some(pos) = lower.find(phrase) {
            let (target_year, target_month) = if now.month() == 1 {
                (now.year() - 1, 12)
            } else {
                (now.year(), now.month() - 1)
            };
            let last_day = days_in_month(target_year, target_month);
            if let (Some(start), Some(end)) = (
                Utc.with_ymd_and_hms(target_year, target_month, 1, 0, 0, 0)
                    .single(),
                Utc.with_ymd_and_hms(target_year, target_month, last_day, 23, 59, 59)
                    .single(),
            ) {
                after = Some(start);
                before = Some(end);
                matched_spans.push((pos, pos + phrase.len()));
            }
            break;
        }
    }

    // "this year"
    if let Some(pos) = lower.find("this year")
        && let (Some(start), Some(end)) = (
            Utc.with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0).single(),
            Utc.with_ymd_and_hms(now.year(), 12, 31, 23, 59, 59)
                .single(),
        )
    {
        after = Some(start);
        before = Some(end);
        matched_spans.push((pos, pos + 9));
    }

    // "last year" / "past year"
    for phrase in &["last year", "past year", "previous year"] {
        if let Some(pos) = lower.find(phrase) {
            let target_year = now.year() - 1;
            if let (Some(start), Some(end)) = (
                Utc.with_ymd_and_hms(target_year, 1, 1, 0, 0, 0).single(),
                Utc.with_ymd_and_hms(target_year, 12, 31, 23, 59, 59)
                    .single(),
            ) {
                after = Some(start);
                before = Some(end);
                matched_spans.push((pos, pos + phrase.len()));
            }
            break;
        }
    }

    // "past N days" / "last N days"
    for prefix in &["past ", "last "] {
        let mut search_pos = 0;
        while let Some(idx) = lower[search_pos..].find(prefix) {
            let abs_pos = search_pos + idx;
            let rem = &lower[abs_pos + prefix.len()..];
            let mut words = rem.split_whitespace();
            if let (Some(num_str), Some(unit)) = (words.next(), words.next())
                && let Ok(n) = num_str.parse::<i64>()
            {
                let unit_clean = unit.trim_matches(|c: char| !c.is_alphabetic());
                let matched_len = prefix.len() + num_str.len() + 1 + unit.len();
                if unit_clean.starts_with("day") {
                    after = Some(now - Duration::days(n));
                    before = Some(now);
                    matched_spans.push((abs_pos, abs_pos + matched_len));
                } else if unit_clean.starts_with("week") {
                    after = Some(now - Duration::days(n * 7));
                    before = Some(now);
                    matched_spans.push((abs_pos, abs_pos + matched_len));
                } else if unit_clean.starts_with("month") {
                    after = Some(now - Duration::days(n * 30));
                    before = Some(now);
                    matched_spans.push((abs_pos, abs_pos + matched_len));
                } else if unit_clean.starts_with("year") {
                    after = Some(now - Duration::days(n * 365));
                    before = Some(now);
                    matched_spans.push((abs_pos, abs_pos + matched_len));
                }
            }
            search_pos = abs_pos + prefix.len();
        }
    }

    // Specific Month parsing: "in January", "in March", "during August", "from July"
    let months = [
        ("january", 1),
        ("jan", 1),
        ("february", 2),
        ("feb", 2),
        ("march", 3),
        ("mar", 3),
        ("april", 4),
        ("apr", 4),
        ("may", 5),
        ("june", 6),
        ("jun", 6),
        ("july", 7),
        ("jul", 7),
        ("august", 8),
        ("aug", 8),
        ("september", 9),
        ("sep", 9),
        ("sept", 9),
        ("october", 10),
        ("oct", 10),
        ("november", 11),
        ("nov", 11),
        ("december", 12),
        ("dec", 12),
    ];

    for &(m_name, m_num) in &months {
        let prefixes = ["in ", "during ", "from ", "since "];
        for prefix in &prefixes {
            let pattern = format!("{}{}", prefix, m_name);
            if let Some(pos) = lower.find(&pattern)
                && !overlaps(pos, pos + pattern.len(), &matched_spans)
            {
                let prev_ok = pos == 0 || !lower.as_bytes()[pos - 1].is_ascii_alphanumeric();
                let end_pos = pos + pattern.len();
                let next_ok =
                    end_pos == lower.len() || !lower.as_bytes()[end_pos].is_ascii_alphanumeric();

                if prev_ok && next_ok {
                    // Check if year follows e.g. "in March 2024"
                    let rem = &lower[end_pos..];
                    let mut target_year = now.year();
                    let mut span_end = end_pos;

                    let next_word = rem.split_whitespace().next().unwrap_or("");
                    let clean_next = next_word.trim_matches(|c: char| !c.is_numeric());
                    if clean_next.len() == 4
                        && let Ok(y) = clean_next.parse::<i32>()
                        && (1990..=2099).contains(&y)
                    {
                        target_year = y;
                        if let Some(w_pos) = rem.find(next_word) {
                            span_end = end_pos + w_pos + next_word.len();
                        }
                    } else if m_num > now.month() && target_year == now.year() {
                        // If month is in future for current year, assume previous year
                        target_year -= 1;
                    }

                    let last_day = days_in_month(target_year, m_num);
                    if let (Some(start), Some(end)) = (
                        Utc.with_ymd_and_hms(target_year, m_num, 1, 0, 0, 0)
                            .single(),
                        Utc.with_ymd_and_hms(target_year, m_num, last_day, 23, 59, 59)
                            .single(),
                    ) {
                        if *prefix == "since " || *prefix == "from " {
                            after = Some(start);
                        } else {
                            after = Some(start);
                            before = Some(end);
                        }
                        matched_spans.push((pos, span_end));
                    }
                }
            }
        }
    }

    // Specific Year parsing: "in 2024", "in 2025", "for 2026", "from 2023", "since 2022", "during 2024"
    for prefix in &["in ", "for ", "from ", "since ", "year ", "during "] {
        let mut search_idx = 0;
        while let Some(idx) = lower[search_idx..].find(prefix) {
            let abs_pos = search_idx + idx;
            let rem = &lower[abs_pos + prefix.len()..];
            let first_token = rem.split_whitespace().next().unwrap_or("");
            let clean_num = first_token.trim_matches(|c: char| !c.is_numeric());
            if clean_num.len() == 4
                && let Ok(year) = clean_num.parse::<i32>()
                && (1990..=2099).contains(&year)
                && !overlaps(
                    abs_pos,
                    abs_pos + prefix.len() + first_token.len(),
                    &matched_spans,
                )
            {
                let span_len = prefix.len() + first_token.len();
                if let (Some(start), Some(end)) = (
                    Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).single(),
                    Utc.with_ymd_and_hms(year, 12, 31, 23, 59, 59).single(),
                ) {
                    if *prefix == "since " || *prefix == "from " {
                        after = Some(start);
                    } else {
                        after = Some(start);
                        before = Some(end);
                    }
                    matched_spans.push((abs_pos, abs_pos + span_len));
                }
            }
            search_idx = abs_pos + prefix.len();
        }
    }

    // "after <date>" / "before <date>"
    for (prefix, is_after) in &[
        ("after ", true),
        ("since ", true),
        ("before ", false),
        ("until ", false),
    ] {
        let mut search_idx = 0;
        while let Some(idx) = lower[search_idx..].find(prefix) {
            let abs_pos = search_idx + idx;
            let rem = &lower[abs_pos + prefix.len()..];
            let token = rem.split_whitespace().next().unwrap_or("");
            let clean_date =
                token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '/');
            if let Some(parsed_dt) = parse_iso_date(clean_date) {
                let span_len = prefix.len() + token.len();
                if !overlaps(abs_pos, abs_pos + span_len, &matched_spans) {
                    if *is_after {
                        after = Some(start_of_day(parsed_dt));
                    } else {
                        before = Some(end_of_day(parsed_dt));
                    }
                    matched_spans.push((abs_pos, abs_pos + span_len));
                }
            }
            search_idx = abs_pos + prefix.len();
        }
    }

    // 5. Clean query text (strip filler words and matched filter tokens)
    let cleaned_text = extract_clean_text(
        trimmed,
        &matched_spans,
        is_project_query,
        is_screenshot_query,
    );

    ParsedQuery {
        raw: trimmed.to_string(),
        text: cleaned_text,
        file_types,
        after,
        before,
        location_hints,
        tag_filters,
        is_project_query,
        is_screenshot_query,
        is_visual_query,
        use_ctime,
    }
}

/// Helper to parse standard numeric date strings: YYYY-MM-DD, YYYY/MM/DD, MM/DD/YYYY, MM-DD-YYYY.
fn parse_iso_date(s: &str) -> Option<DateTime<Utc>> {
    // YYYY-MM-DD or YYYY/MM/DD
    let parts: Vec<&str> = if s.contains('-') {
        s.split('-').collect()
    } else if s.contains('/') {
        s.split('/').collect()
    } else {
        return None;
    };

    if parts.len() != 3 {
        return None;
    }

    if parts[0].len() == 4 {
        // YYYY-MM-DD
        let y: i32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let d: u32 = parts[2].parse().ok()?;
        if (1..=12).contains(&m) && (1..=31).contains(&d) {
            return Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).single();
        }
    } else if parts[2].len() == 4 {
        // MM/DD/YYYY or DD/MM/YYYY
        let p1: u32 = parts[0].parse().ok()?;
        let p2: u32 = parts[1].parse().ok()?;
        let y: i32 = parts[2].parse().ok()?;
        if (1..=12).contains(&p1) && (1..=31).contains(&p2) {
            return Utc.with_ymd_and_hms(y, p1, p2, 0, 0, 0).single();
        }
    }

    None
}

/// Helper to find a whole word in lowercase string.
fn find_word(haystack: &str, word: &str) -> Option<usize> {
    let mut search_idx = 0;
    while let Some(found_idx) = haystack[search_idx..].find(word) {
        let abs_idx = search_idx + found_idx;
        let end_idx = abs_idx + word.len();

        let prev_ok = abs_idx == 0 || !haystack.as_bytes()[abs_idx - 1].is_ascii_alphanumeric();
        let next_ok =
            end_idx == haystack.len() || !haystack.as_bytes()[end_idx].is_ascii_alphanumeric();

        if prev_ok && next_ok {
            return Some(abs_idx);
        }
        search_idx = end_idx;
    }
    None
}

/// Extract clean search text by removing matched filter spans and conversational filler words.
fn extract_clean_text(
    raw: &str,
    matched_spans: &[(usize, usize)],
    is_project: bool,
    is_screenshot: bool,
) -> String {
    // 1. Build a mask of characters that belong to matched filter spans
    let bytes = raw.as_bytes();
    let mut keep_mask = vec![true; raw.len()];

    for &(start, end) in matched_spans {
        for item in keep_mask.iter_mut().take(end.min(raw.len())).skip(start) {
            *item = false;
        }
    }

    // 2. Reconstruct string from unmasked bytes
    let mut unmasked_chars = String::new();
    for (i, &b) in bytes.iter().enumerate() {
        if keep_mask[i] {
            unmasked_chars.push(b as char);
        } else {
            unmasked_chars.push(' ');
        }
    }

    // 3. Tokenize and filter conversational filler phrases/words
    let filler_words = [
        "find",
        "that",
        "the",
        "a",
        "an",
        "i",
        "me",
        "my",
        "show",
        "get",
        "give",
        "look",
        "looking",
        "search",
        "where",
        "where's",
        "is",
        "about",
        "with",
        "for",
        "all",
        "on",
        "in",
        "from",
        "to",
        "under",
        "downloaded",
        "downloads",
        "desktop",
        "documents",
        "folder",
        "directory",
        "file",
        "files",
        "doc",
        "docs",
        "document",
        "documents",
        "pdf",
        "pdfs",
        "screenshot",
        "screenshots",
        "photo",
        "photos",
        "picture",
        "pictures",
        "image",
        "images",
        "containing",
        "contains",
        "with",
        "having",
        "project",
        "projects",
        "repo",
        "repos",
        "repository",
        "workspace",
    ];

    let mut clean_tokens = Vec::new();
    for token in unmasked_chars.split_whitespace() {
        let clean = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
        let lower = clean.to_lowercase();
        if clean.is_empty() {
            continue;
        }

        // If it's a project query and token is "project" or "repo", skip it
        if is_project
            && (lower == "project"
                || lower == "repo"
                || lower == "repository"
                || lower == "workspace")
        {
            continue;
        }

        // If it's a screenshot query and token is "screenshot", skip it
        if is_screenshot && (lower == "screenshot" || lower == "screenshots" || lower == "snip") {
            continue;
        }

        if !filler_words.contains(&lower.as_str()) {
            clean_tokens.push(clean.to_string());
        }
    }

    clean_tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ref_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 22, 12, 0, 0).unwrap()
    }

    #[test]
    fn test_parse_pdf_and_date() {
        let now = test_ref_time();
        let q =
            parse_query_with_ref_time("find that pdf about AI agents I downloaded last month", now);
        assert_eq!(q.file_types, vec!["pdf"]);
        assert_eq!(q.location_hints, vec!["downloads"]);
        assert!(q.use_ctime);
        assert_eq!(q.text, "AI agents");
        assert_eq!(
            q.after,
            Some(Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap())
        );
        assert_eq!(
            q.before,
            Some(Utc.with_ymd_and_hms(2026, 8, 31, 23, 59, 59).unwrap())
        );
    }

    #[test]
    fn test_parse_project_query() {
        let now = test_ref_time();
        let q = parse_query_with_ref_time("where is my NutriGrade project", now);
        assert!(q.is_project_query);
        assert_eq!(q.text, "NutriGrade");
    }

    #[test]
    fn test_parse_relative_days() {
        let now = test_ref_time();
        let q = parse_query_with_ref_time("notes from past 3 days", now);
        assert_eq!(q.file_types, vec!["md", "txt", "org"]);
        assert_eq!(q.after, Some(now - Duration::days(3)));
        assert_eq!(q.before, Some(now));
    }

    #[test]
    fn test_parse_screenshot_and_qr_queries() {
        let now = test_ref_time();
        let q = parse_query_with_ref_time("screenshots with a payment QR", now);
        assert!(q.is_screenshot_query);
        assert!(q.is_visual_query);
        assert!(q.tag_filters.contains(&"type:screenshot".to_string()));
        assert!(q.tag_filters.contains(&"qr:payment".to_string()));
        assert!(q.file_types.contains(&"png".to_string()));

        let q2 = parse_query_with_ref_time("wifi qr code", now);
        assert!(q2.is_visual_query);
        assert!(q2.tag_filters.contains(&"qr:wifi".to_string()));

        let q3 = parse_query_with_ref_time("photo of a whiteboard", now);
        assert!(q3.is_visual_query);
        assert!(q3.tag_filters.contains(&"type:photo".to_string()));
        assert_eq!(q3.text, "whiteboard");

        let q4 = parse_query_with_ref_time("receipt from last month", now);
        assert!(q4.is_visual_query);
        assert_eq!(q4.text, "receipt");
        assert!(q4.after.is_some());
    }
}
