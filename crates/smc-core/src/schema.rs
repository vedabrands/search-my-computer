//! Table and column name constants to avoid string duplication.

pub const TABLE_FILES: &str = "files";
pub const TABLE_CHUNKS: &str = "chunks";
pub const TABLE_JOBS: &str = "jobs";
pub const TABLE_META: &str = "meta";
pub const TABLE_FILES_FTS: &str = "files_fts";
pub const TABLE_CHUNKS_FTS: &str = "chunks_fts";
pub const TABLE_CHUNK_VECTORS: &str = "chunk_vectors";
pub const TABLE_PROJECTS: &str = "projects";
pub const TABLE_PROJECTS_FTS: &str = "projects_fts";
pub const TABLE_IMAGE_METADATA: &str = "image_metadata";
pub const TABLE_IMAGE_TAGS: &str = "image_tags";
pub const TABLE_IMAGE_VECTORS: &str = "image_vectors";

/// File status values stored in `files.status`.
pub mod file_status {
    pub const ACTIVE: &str = "active";
    pub const DELETED: &str = "deleted";
    pub const ERROR: &str = "error";
}

/// Job state values stored in `jobs.state`.
pub mod job_state {
    pub const PENDING: &str = "pending";
    pub const RUNNING: &str = "running";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
}

/// Job kind values stored in `jobs.kind`.
pub mod job_kind {
    pub const INDEX_FILENAME: &str = "index_filename";
    pub const EXTRACT: &str = "extract";
    pub const EMBED: &str = "embed";
    pub const VISION: &str = "vision";
}

/// File kind classification.
pub mod file_kind {
    pub const DOCUMENT: &str = "document";
    pub const CODE: &str = "code";
    pub const IMAGE: &str = "image";
    pub const ARCHIVE: &str = "archive";
    pub const OTHER: &str = "other";

    /// Classify a file by extension.
    pub fn classify(ext: &str) -> &'static str {
        match ext.to_lowercase().as_str() {
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
            | "rtf" | "txt" | "md" | "csv" | "tsv" | "json" | "xml" | "yaml" | "yml" | "toml"
            | "ini" | "cfg" | "conf" | "log" | "tex" | "epub" => DOCUMENT,

            "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "kt" | "c" | "cpp" | "h"
            | "hpp" | "cs" | "go" | "rb" | "php" | "swift" | "m" | "scala" | "clj" | "hs"
            | "erl" | "ex" | "exs" | "lua" | "r" | "jl" | "sql" | "sh" | "bash" | "zsh" | "ps1"
            | "bat" | "cmd" | "html" | "css" | "scss" | "sass" | "less" | "vue" | "svelte"
            | "zig" | "nim" | "dart" | "v" | "wasm" | "proto" | "graphql" => CODE,

            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff" | "tif"
            | "raw" | "heic" | "heif" | "avif" => IMAGE,

            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => ARCHIVE,

            _ => OTHER,
        }
    }
}
