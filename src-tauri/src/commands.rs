use crate::state::AppState;
use rusqlite::OptionalExtension;
use serde::Serialize;
use smc_core::config::AppConfig;
use smc_core::db::ProjectRecord;
use smc_core::jobs::JobStatusCounts;
use smc_core::scanner::{ScanSnapshot, Scanner};
use smc_license::license::{
    EMBEDDED_PUBLIC_KEY, LicenseStatus, MAJOR_VERSION, PRODUCT_NAME, verify_license,
};
use smc_nlq::ParsedQuery;
use smc_search::RankingConfig;
use smc_search::hybrid::SearchResult;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{error, info, warn};

use smc_embed::embedder::Embedder;

#[derive(Debug, Serialize)]
pub struct IndexStatus {
    pub total_files: i64,
    pub active_files: i64,
    pub is_scanning: bool,
    pub indexing_paused: bool,
    pub scan_progress: ScanSnapshot,
    pub job_counts: JobStatusCounts,
    pub indexed_folders: Vec<String>,
    pub hotkey_registered: bool,
    pub has_embedding_model: bool,
    pub embedding_model_id: Option<String>,
    pub enable_image_indexing: bool,
    pub image_folders: Vec<String>,
    pub has_vision_models: bool,
    pub wake_word_enabled: bool,
    pub wake_word_phrase: String,
    pub is_wake_word_paused: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct DetectedFolder {
    pub name: String,
    pub path: String,
    pub category: String, // "standard", "project", "media"
    pub is_sensitive: bool,
    pub exists: bool,
    pub default_checked: bool,
}

#[derive(Debug, Serialize)]
pub struct ExclusionTestResult {
    pub matches: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FilePreviewResponse {
    pub file_id: Option<i64>,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub kind: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub content_preview: Option<String>,
    pub language: Option<String>,
    pub line_count: Option<usize>,
    pub is_truncated: bool,
    pub image_meta: Option<ImageDetailsResponse>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticsExportResult {
    pub file_path: String,
    pub size_bytes: u64,
    pub exported_at: String,
}

#[derive(Debug, Serialize)]
pub struct ImageTagDetails {
    pub tag: String,
    pub masked_payload: Option<String>,
    pub raw_payload: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ImageDetailsResponse {
    pub file_id: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub format: Option<String>,
    pub exif_date: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub is_screenshot: bool,
    pub has_qr: bool,
    pub qr_count: i64,
    pub tags: Vec<ImageTagDetails>,
    pub ocr_text: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let limit = limit.unwrap_or(20);
    let mut results = smc_search::hybrid_search_nlq_vision(
        &state.db,
        state.embedder.as_deref().map(|e| e as &dyn Embedder),
        Some(state.vector_index.as_ref() as &dyn smc_embed::vector_index::VectorIndex),
        Some(state.vision_pipeline.clip_engine()),
        &query,
        limit,
        &RankingConfig::default(),
    )
    .map_err(|e| e.to_string())?;

    // Trial-expired gate: restrict to top 3 results.
    let status = state.license_status.lock().clone();
    if status.is_expired_trial() {
        results.truncate(3);
    }

    Ok(results)
}

#[tauri::command]
pub async fn get_image_details(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Option<ImageDetailsResponse>, String> {
    let reader = state.db.reader().map_err(|e| e.to_string())?;

    let file_info: Option<(String, String)> = reader
        .query_row(
            "SELECT path, status FROM files WHERE id = ?1",
            rusqlite::params![file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (path_str, status) = match file_info {
        Some(info) => info,
        None => return Ok(None),
    };

    if status == "deleted" {
        return Ok(None);
    }

    struct RawImageMeta {
        width: i64,
        height: i64,
        format: String,
        exif_date: Option<String>,
        camera_make: Option<String>,
        camera_model: Option<String>,
        is_screenshot: bool,
        has_qr: bool,
        qr_count: i64,
    }

    let meta_row: Option<RawImageMeta> = reader
        .query_row(
            "SELECT width, height, format, exif_date, camera_make, camera_model, is_screenshot, has_qr, qr_count
             FROM image_metadata WHERE file_id = ?1",
            rusqlite::params![file_id],
            |row| {
                Ok(RawImageMeta {
                    width: row.get(0)?,
                    height: row.get(1)?,
                    format: row.get(2)?,
                    exif_date: row.get(3)?,
                    camera_make: row.get(4)?,
                    camera_model: row.get(5)?,
                    is_screenshot: row.get(6)?,
                    has_qr: row.get(7)?,
                    qr_count: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let mut stmt = reader
        .prepare("SELECT tag, payload FROM image_tags WHERE file_id = ?1")
        .map_err(|e| e.to_string())?;

    let tag_rows = stmt
        .query_map(rusqlite::params![file_id], |row| {
            Ok(ImageTagDetails {
                tag: row.get(0)?,
                masked_payload: row.get(1)?,
                raw_payload: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut tags = Vec::new();
    for item in tag_rows.flatten() {
        tags.push(item);
    }

    let mut chunk_stmt = reader
        .prepare("SELECT text FROM chunks WHERE file_id = ?1 ORDER BY ordinal ASC")
        .map_err(|e| e.to_string())?;
    let chunk_rows = chunk_stmt
        .query_map(rusqlite::params![file_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut ocr_chunks = Vec::new();
    for txt in chunk_rows.flatten() {
        ocr_chunks.push(txt);
    }
    let ocr_text = if !ocr_chunks.is_empty() {
        Some(ocr_chunks.join("\n\n"))
    } else {
        None
    };

    let p = Path::new(&path_str);
    let thumbnail_path = state
        .vision_pipeline
        .thumbnail_manager()
        .thumbnail_path(p, None);
    let thumbnail_path_str = if thumbnail_path.exists() {
        Some(thumbnail_path.to_string_lossy().to_string())
    } else {
        None
    };

    if let Some(meta) = meta_row {
        Ok(Some(ImageDetailsResponse {
            file_id,
            width: Some(meta.width),
            height: Some(meta.height),
            format: Some(meta.format),
            exif_date: meta.exif_date,
            camera_make: meta.camera_make,
            camera_model: meta.camera_model,
            is_screenshot: meta.is_screenshot,
            has_qr: meta.has_qr,
            qr_count: meta.qr_count,
            tags,
            ocr_text,
            thumbnail_path: thumbnail_path_str,
        }))
    } else {
        Ok(Some(ImageDetailsResponse {
            file_id,
            width: None,
            height: None,
            format: None,
            exif_date: None,
            camera_make: None,
            camera_model: None,
            is_screenshot: false,
            has_qr: false,
            qr_count: 0,
            tags,
            ocr_text,
            thumbnail_path: thumbnail_path_str,
        }))
    }
}

#[tauri::command]
pub async fn parse_nlq(query: String) -> Result<ParsedQuery, String> {
    Ok(smc_nlq::parse_query(&query))
}

#[tauri::command]
pub async fn search_projects(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<ProjectRecord>, String> {
    let limit = limit.unwrap_or(10);
    state
        .db
        .search_projects(&query, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("path does not exist: {path}"));
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let win_path = path.replace('/', "\\");
        Command::new("explorer")
            .args(["/select,", &win_path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let parent = if p.is_file() {
            p.parent().unwrap_or(p)
        } else {
            p
        };
        Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn open_terminal_in_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let dir = if p.is_file() {
        p.parent().unwrap_or(p)
    } else {
        p
    };

    if !dir.exists() {
        return Err(format!("directory does not exist: {}", dir.display()));
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let win_dir = dir.to_string_lossy().replace('/', "\\");
        let safe_win_dir = win_dir.replace('\'', "''");
        Command::new("cmd")
            .args([
                "/C",
                "start",
                "powershell",
                "-NoExit",
                "-Command",
                &format!("Set-Location -LiteralPath '{}'", safe_win_dir),
            ])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .args(["-a", "Terminal", &dir.to_string_lossy()])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        Command::new("x-terminal-emulator")
            .args(["--working-directory", &dir.to_string_lossy()])
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .args(["--working-directory", &dir.to_string_lossy()])
                    .spawn()
            })
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn semantic_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    let limit = limit.unwrap_or(20);
    if let Some(ref embedder) = state.embedder {
        smc_search::semantic_search(
            &state.db,
            embedder.as_ref(),
            state.vector_index.as_ref(),
            &query,
            limit,
        )
        .map_err(|e| e.to_string())
    } else {
        Err("Semantic search is unavailable because embedding model files are missing.".into())
    }
}

#[tauri::command]
pub async fn add_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_path: String,
) -> Result<Vec<String>, String> {
    let normalized = folder_path.replace('\\', "/");
    let path = Path::new(&normalized);

    if !path.exists() || !path.is_dir() {
        return Err(format!("path is not a valid directory: {folder_path}"));
    }

    let mut config = state.config.lock();
    if !config.indexed_folders.contains(&normalized) {
        config.indexed_folders.push(normalized);
        config.save().map_err(|e| e.to_string())?;
    }

    let folders = config.indexed_folders.clone();
    drop(config);

    // Auto-trigger a scan of the new folder in the background.
    trigger_scan(app, state.inner().clone());

    Ok(folders)
}

#[tauri::command]
pub async fn remove_folder(
    state: State<'_, AppState>,
    folder_path: String,
) -> Result<Vec<String>, String> {
    let normalized = folder_path.replace('\\', "/");

    let mut config = state.config.lock();
    config.indexed_folders.retain(|f| f != &normalized);
    config.save().map_err(|e| e.to_string())?;

    let folders = config.indexed_folders.clone();
    drop(config);

    // Mark files under that folder as deleted.
    let prefix = if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    };

    let conn = state.db.writer();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE files SET status = 'deleted', last_indexed_at = ?1 WHERE path LIKE ?2",
        rusqlite::params![now, format!("{prefix}%")],
    )
    .map_err(|e| e.to_string())?;

    Ok(folders)
}

#[tauri::command]
pub async fn start_scan(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.is_scanning.load(Ordering::Relaxed) {
        return Err("scan already in progress".into());
    }
    trigger_scan(app, state.inner().clone());
    Ok(())
}

#[tauri::command]
pub async fn get_index_status(state: State<'_, AppState>) -> Result<IndexStatus, String> {
    let reader = state.db.reader().map_err(|e| e.to_string())?;

    let total_files: i64 = reader
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap_or(0);

    let active_files: i64 = reader
        .query_row(
            "SELECT COUNT(*) FROM files WHERE status = 'active'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let config = state.config.lock();
    let indexed_folders = config.indexed_folders.clone();
    let indexing_paused = config.indexing_paused;
    let enable_image_indexing = config.enable_image_indexing;
    let image_folders = config.image_folders.clone();
    let wake_word_enabled = config.wake_word_enabled;
    let wake_word_phrase = config.wake_word_phrase.clone();
    drop(config);

    let is_wake_word_paused = state.voice_manager.is_wake_word_paused();
    let job_counts = state.job_queue.status_counts().unwrap_or_default();

    let has_embedding_model = state.embedder.is_some();
    let embedding_model_id = state.embedder.as_ref().map(|e| e.model_id().to_string());
    let has_vision_models = state.vision_pipeline.clip_engine().is_available();

    Ok(IndexStatus {
        total_files,
        active_files,
        is_scanning: state.is_scanning.load(Ordering::Relaxed),
        indexing_paused,
        scan_progress: state.scan_progress.snapshot(),
        job_counts,
        indexed_folders,
        hotkey_registered: state.hotkey_registered.load(Ordering::Relaxed),
        has_embedding_model,
        embedding_model_id,
        enable_image_indexing,
        image_folders,
        has_vision_models,
        wake_word_enabled,
        wake_word_phrase,
        is_wake_word_paused,
    })
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().clone())
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, AppState>,
    new_config: AppConfig,
) -> Result<(), String> {
    let issues = new_config.validate();
    if !issues.is_empty() {
        return Err(format!("invalid config: {}", issues.join(", ")));
    }

    let mut config = state.config.lock();
    *config = new_config;
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn pause_indexing(
    state: State<'_, AppState>,
    duration_secs: Option<u64>,
) -> Result<(), String> {
    if let Some(secs) = duration_secs {
        state
            .governor
            .pause_for(std::time::Duration::from_secs(secs));
    } else {
        state.governor.pause_until_restart();
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_indexing(state: State<'_, AppState>) -> Result<(), String> {
    state.governor.resume();
    Ok(())
}

#[tauri::command]
pub async fn get_governor_status(state: State<'_, AppState>) -> Result<String, String> {
    let pending = state.job_queue.pending_count().unwrap_or(0) as usize;
    Ok(state.governor.status_label(pending))
}

#[tauri::command]
pub async fn get_problem_files(
    state: State<'_, AppState>,
    unresolved_only: Option<bool>,
) -> Result<Vec<smc_core::health::ProblemFileRecord>, String> {
    smc_core::health::list_problem_files(&state.db, unresolved_only.unwrap_or(true))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_launch_at_login(state: State<'_, AppState>, enable: bool) -> Result<(), String> {
    let mut config = state.config.lock();
    config.launch_at_login = enable;
    config.save().map_err(|e| e.to_string())?;
    drop(config);
    AppConfig::set_launch_at_login(enable).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn detect_system_folders() -> Result<Vec<DetectedFolder>, String> {
    let mut folders = Vec::new();

    if let Some(user_dirs) = directories::UserDirs::new() {
        // Documents
        if let Some(p) = user_dirs.document_dir() {
            let path_str = p.to_string_lossy().replace('\\', "/");
            folders.push(DetectedFolder {
                name: "Documents".into(),
                path: path_str,
                category: "standard".into(),
                is_sensitive: false,
                exists: p.exists(),
                default_checked: true,
            });
        }

        // Downloads
        if let Some(p) = user_dirs.download_dir() {
            let path_str = p.to_string_lossy().replace('\\', "/");
            folders.push(DetectedFolder {
                name: "Downloads".into(),
                path: path_str,
                category: "standard".into(),
                is_sensitive: false,
                exists: p.exists(),
                default_checked: true,
            });
        }

        // Desktop
        if let Some(p) = user_dirs.desktop_dir() {
            let path_str = p.to_string_lossy().replace('\\', "/");
            folders.push(DetectedFolder {
                name: "Desktop".into(),
                path: path_str,
                category: "standard".into(),
                is_sensitive: false,
                exists: p.exists(),
                default_checked: true,
            });
        }

        // Home dev / project directories check
        let home = user_dirs.home_dir();
        let project_candidates = [
            ("Projects", home.join("Projects")),
            ("Dev", home.join("dev")),
            ("Code", home.join("code")),
            ("Workspace", home.join("workspace")),
            ("Source Repos", home.join("source").join("repos")),
            ("Src", home.join("src")),
        ];

        for (name, path) in project_candidates {
            if path.exists() && path.is_dir() {
                let path_str = path.to_string_lossy().replace('\\', "/");
                folders.push(DetectedFolder {
                    name: format!("Projects ({})", name),
                    path: path_str,
                    category: "project".into(),
                    is_sensitive: false,
                    exists: true,
                    default_checked: true,
                });
            }
        }

        // Pictures (Media / Sensitive - default unchecked)
        if let Some(p) = user_dirs.picture_dir() {
            let path_str = p.to_string_lossy().replace('\\', "/");
            folders.push(DetectedFolder {
                name: "Pictures (Photos & Screenshots)".into(),
                path: path_str,
                category: "media".into(),
                is_sensitive: true,
                exists: p.exists(),
                default_checked: false,
            });
        }

        // Videos (Media - default unchecked)
        if let Some(p) = user_dirs.video_dir() {
            let path_str = p.to_string_lossy().replace('\\', "/");
            folders.push(DetectedFolder {
                name: "Videos".into(),
                path: path_str,
                category: "media".into(),
                is_sensitive: true,
                exists: p.exists(),
                default_checked: false,
            });
        }
    }

    Ok(folders)
}

#[tauri::command]
pub async fn test_exclusion_pattern(
    pattern: String,
    test_path: String,
) -> Result<ExclusionTestResult, String> {
    match globset::Glob::new(&pattern) {
        Ok(glob) => {
            let matcher = glob.compile_matcher();
            let norm_path = test_path.replace('\\', "/");
            let is_match = matcher.is_match(&norm_path);
            Ok(ExclusionTestResult {
                matches: is_match,
                error: None,
            })
        }
        Err(e) => Ok(ExclusionTestResult {
            matches: false,
            error: Some(e.to_string()),
        }),
    }
}

#[tauri::command]
pub async fn get_file_preview(
    state: State<'_, AppState>,
    file_id: Option<i64>,
    path: Option<String>,
) -> Result<Option<FilePreviewResponse>, String> {
    let resolved_path = if let Some(fid) = file_id {
        let reader = state.db.reader().map_err(|e| e.to_string())?;
        reader
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                rusqlite::params![fid],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
    } else {
        path
    };

    let p_str = match resolved_path {
        Some(p) => p,
        None => return Ok(None),
    };

    let p = Path::new(&p_str);
    let filename = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = p
        .extension()
        .map(|e| e.to_string_lossy().to_string().to_lowercase())
        .unwrap_or_default();

    let meta = std::fs::metadata(p).ok();
    let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified_at = meta.as_ref().and_then(|m| m.modified().ok()).map(|t| {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        dt.to_rfc3339()
    });

    let is_image = matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "ico" | "svg"
    );
    let kind = if is_image {
        "image"
    } else if matches!(
        extension.as_str(),
        "pdf" | "docx" | "doc" | "pptx" | "xlsx" | "csv" | "txt" | "md" | "rtf"
    ) {
        "document"
    } else if matches!(
        extension.as_str(),
        "rs" | "py"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "html"
            | "css"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "sql"
            | "sh"
            | "bat"
            | "ps1"
    ) {
        "code"
    } else {
        "other"
    };

    let language = match extension.as_str() {
        "rs" => Some("rust".into()),
        "py" => Some("python".into()),
        "ts" => Some("typescript".into()),
        "tsx" => Some("typescript".into()),
        "js" => Some("javascript".into()),
        "jsx" => Some("javascript".into()),
        "json" => Some("json".into()),
        "md" => Some("markdown".into()),
        "html" => Some("html".into()),
        "css" => Some("css".into()),
        "toml" => Some("toml".into()),
        "yaml" | "yml" => Some("yaml".into()),
        "sql" => Some("sql".into()),
        "sh" => Some("bash".into()),
        "ps1" => Some("powershell".into()),
        "c" | "h" => Some("c".into()),
        "cpp" | "hpp" => Some("cpp".into()),
        "go" => Some("go".into()),
        "java" => Some("java".into()),
        _ => None,
    };

    let image_meta = if is_image {
        if let Some(fid) = file_id {
            get_image_details(state.clone(), fid).await.unwrap_or(None)
        } else {
            let reader = state.db.reader().map_err(|e| e.to_string())?;
            let fid: Option<i64> = reader
                .query_row(
                    "SELECT id FROM files WHERE path = ?1",
                    rusqlite::params![p_str],
                    |r| r.get(0),
                )
                .optional()
                .unwrap_or(None);
            if let Some(fid) = fid {
                get_image_details(state.clone(), fid).await.unwrap_or(None)
            } else {
                None
            }
        }
    } else {
        None
    };

    let mut content_preview = None;
    let mut line_count = None;
    let mut is_truncated = false;

    if !is_image
        && p.exists()
        && p.is_file()
        && size_bytes <= 10 * 1024 * 1024
        && let Ok(file) = std::fs::File::open(p)
    {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(file);
        let mut preview_lines = Vec::new();
        let mut line = String::new();
        let mut total_lines = 0;
        let mut total_bytes = 0;

        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            total_lines += 1;
            if preview_lines.len() < 200 && total_bytes < 32768 {
                total_bytes += n;
                preview_lines.push(line.clone());
            } else {
                is_truncated = true;
            }
            line.clear();
        }
        line_count = Some(total_lines);
        content_preview = Some(preview_lines.join(""));
    }

    Ok(Some(FilePreviewResponse {
        file_id,
        path: p_str,
        filename,
        extension,
        kind: kind.into(),
        size_bytes,
        modified_at,
        content_preview,
        language,
        line_count,
        is_truncated,
        image_meta,
    }))
}

#[tauri::command]
pub async fn delete_all_data(state: State<'_, AppState>) -> Result<(), String> {
    state.governor.pause_until_restart();
    state.db.clear_all_data().map_err(|e| e.to_string())?;

    let mut config = state.config.lock();
    config.indexed_folders.clear();
    config.image_folders.clear();
    config.save().map_err(|e| e.to_string())?;

    info!("all indexed user data successfully deleted");
    Ok(())
}

#[tauri::command]
pub async fn rebuild_index(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.db.truncate_index_data().map_err(|e| e.to_string())?;
    trigger_scan(app, state.inner().clone());
    info!("index rebuild initiated");
    Ok(())
}

#[tauri::command]
pub async fn export_diagnostics(
    state: State<'_, AppState>,
) -> Result<DiagnosticsExportResult, String> {
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let data_dir = AppConfig::data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let file_path = data_dir.join(format!("diagnostics_{}.json", timestamp));

    let index_status = get_index_status(state.clone())
        .await
        .unwrap_or_else(|_| IndexStatus {
            total_files: 0,
            active_files: 0,
            is_scanning: false,
            indexing_paused: false,
            scan_progress: ScanSnapshot::default(),
            job_counts: JobStatusCounts::default(),
            indexed_folders: Vec::new(),
            hotkey_registered: false,
            has_embedding_model: false,
            embedding_model_id: None,
            enable_image_indexing: false,
            image_folders: Vec::new(),
            has_vision_models: false,
            wake_word_enabled: false,
            wake_word_phrase: "Kira".into(),
            is_wake_word_paused: false,
        });

    let config = state.config.lock().clone();
    let problem_files = smc_core::health::list_problem_files(&state.db, false).unwrap_or_default();
    let governor_status = state.governor.status_label(0);

    let report = serde_json::json!({
        "app": "SearchMyComputer",
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": now.to_rfc3339(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "config": {
            "hotkey": config.hotkey,
            "theme": config.theme,
            "index_threads": config.index_threads,
            "max_file_size": config.max_file_size,
            "enable_image_indexing": config.enable_image_indexing,
            "launch_at_login": config.launch_at_login,
            "onboarding_completed": config.onboarding_completed,
            "battery_policy": config.battery_policy,
            "language": config.language,
            "max_ram_mb": config.max_ram_mb,
            "indexed_folders_count": config.indexed_folders.len(),
            "exclusions_count": config.exclusions.len(),
        },
        "index_status": index_status,
        "problem_files_count": problem_files.len(),
        "problem_files_summary": problem_files.iter().take(20).collect::<Vec<_>>(),
        "governor": governor_status,
        "privacy_guarantee": "100% local, zero telemetry, zero cloud calls"
    });

    let json_bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&file_path, &json_bytes).map_err(|e| e.to_string())?;

    let path_str = file_path.to_string_lossy().to_string();
    Ok(DiagnosticsExportResult {
        file_path: path_str,
        size_bytes: json_bytes.len() as u64,
        exported_at: now.to_rfc3339(),
    })
}

#[tauri::command]
pub async fn clear_problem_files(state: State<'_, AppState>) -> Result<usize, String> {
    smc_core::health::clear_problem_files(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn retry_problem_files(state: State<'_, AppState>) -> Result<usize, String> {
    let unresolved =
        smc_core::health::list_problem_files(&state.db, true).map_err(|e| e.to_string())?;
    let count = unresolved.len();
    let conn = state.db.writer();
    let now = chrono::Utc::now().to_rfc3339();

    for p in unresolved {
        if let Some(fid) = p.file_id {
            let _ = conn.execute(
                "INSERT INTO jobs (kind, file_id, priority, state, created_at)
                 VALUES (?1, ?2, 10, 'pending', ?3)",
                rusqlite::params![smc_core::schema::job_kind::EXTRACT, fid, now],
            );
        }
    }
    drop(conn);

    let _ = smc_core::health::clear_problem_files(&state.db);
    Ok(count)
}

#[tauri::command]
pub async fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Helper: run a scan across all indexed folders in a background thread.
fn trigger_scan(app: AppHandle, state: AppState) {
    if state.is_scanning.swap(true, Ordering::SeqCst) {
        return;
    }

    let config = state.config.lock().clone();
    let is_scanning = Arc::clone(&state.is_scanning);

    thread::Builder::new()
        .name("smc-scanner".into())
        .spawn(move || {
            info!("background scan thread started");

            for folder in &config.indexed_folders {
                let path = Path::new(folder);
                if !path.exists() {
                    warn!(folder, "indexed folder does not exist, skipping");
                    continue;
                }

                match Scanner::new(state.db.clone(), &config) {
                    Ok(scanner) => {
                        // Forward progress events to UI.
                        let progress_arc = scanner.progress();
                        let app_clone = app.clone();
                        let progress_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let p_cancel = Arc::clone(&progress_cancel);

                        let ticker = thread::spawn(move || {
                            while !p_cancel.load(Ordering::Relaxed) {
                                let snap = progress_arc.snapshot();
                                let _ = app_clone.emit("scan-progress", &snap);
                                thread::sleep(std::time::Duration::from_millis(500));
                            }
                        });

                        let result = scanner.scan_folder(path);
                        progress_cancel.store(true, Ordering::Relaxed);
                        let _ = ticker.join();

                        match result {
                            Ok(snap) => {
                                let _ = app.emit("scan-complete", &snap);
                            }
                            Err(e) => {
                                error!(folder, error = %e, "folder scan failed");
                                let _ = app.emit("scan-error", e.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "cannot create scanner");
                    }
                }
            }

            is_scanning.store(false, Ordering::SeqCst);
            let _ = app.emit("scan-all-complete", ());
            info!("background scan thread finished");
        })
        .ok();
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportLicenseResult {
    pub success: bool,
    pub status: LicenseStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub release_url: String,
}

#[tauri::command]
pub fn get_license_status(state: State<'_, AppState>) -> LicenseStatus {
    state.license_status.lock().clone()
}

#[tauri::command]
pub fn import_license(
    state: State<'_, AppState>,
    license_data_or_path: String,
) -> Result<ImportLicenseResult, String> {
    let input = license_data_or_path.trim();
    let (json_content, saved_path) = if input.starts_with('{') {
        let data_dir = AppConfig::data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
        let lic_file = data_dir.join("license.lic");
        (
            input.to_string(),
            Some(lic_file.to_string_lossy().to_string()),
        )
    } else {
        let path = Path::new(input);
        if !path.exists() {
            return Err(format!("License file not found: {}", input));
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read license file: {e}"))?;
        (content, Some(input.to_string()))
    };

    let lic = verify_license(
        &json_content,
        &EMBEDDED_PUBLIC_KEY,
        PRODUCT_NAME,
        MAJOR_VERSION,
    )
    .map_err(|e| format!("License verification failed: {e}"))?;

    if input.starts_with('{')
        && let Some(ref path_str) = saved_path
    {
        let _ = std::fs::write(path_str, &json_content);
    }

    let new_status = LicenseStatus::Licensed {
        customer_name: lic.customer_name.clone(),
        license_id: lic.license_id.clone(),
    };

    *state.license_status.lock() = new_status.clone();

    if let Some(path_str) = saved_path {
        let mut config = state.config.lock();
        config.license_file_path = Some(path_str);
        if let Err(e) = config.save() {
            warn!(error = %e, "Failed to persist license file path to config");
        }
    }

    info!(
        license_id = %lic.license_id,
        customer = %lic.customer_name,
        "license imported successfully"
    );

    Ok(ImportLicenseResult {
        success: true,
        status: new_status,
        message: format!("License verified for {}", lic.customer_name),
    })
}

#[tauri::command]
pub fn check_for_updates() -> UpdateCheckResult {
    UpdateCheckResult {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        release_url: "https://github.com/searchmycomputer/search-my-computer/releases".to_string(),
    }
}

#[tauri::command]
pub async fn start_voice_capture(state: State<'_, AppState>) -> Result<(), String> {
    // Ensure mic stream is active
    let mut stream_guard = state.mic_stream.lock();
    if stream_guard.is_none() {
        match smc_stt::capture::AudioCaptureManager::spawn_mic_stream(state.voice_manager.clone()) {
            Ok(stream) => {
                *stream_guard = Some(stream);
                info!("microphone stream started for voice recording");
            }
            Err(e) => {
                return Err(format!("Microphone unavailable: {e}"));
            }
        }
    }
    drop(stream_guard);

    state
        .voice_manager
        .start_manual_recording()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_voice_capture(state: State<'_, AppState>) -> Result<String, String> {
    state
        .voice_manager
        .stop_listening_now()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_voice_state(
    state: State<'_, AppState>,
) -> Result<smc_stt::capture::AudioCaptureState, String> {
    Ok(state.voice_manager.get_state())
}

#[tauri::command]
pub async fn pause_wake_word(
    state: State<'_, AppState>,
    duration_secs: Option<u64>,
) -> Result<(), String> {
    let dur = std::time::Duration::from_secs(duration_secs.unwrap_or(3600));
    state.voice_manager.pause_wake_word(dur);
    Ok(())
}

#[tauri::command]
pub async fn resume_wake_word(state: State<'_, AppState>) -> Result<(), String> {
    state.voice_manager.resume_wake_word();
    Ok(())
}

#[tauri::command]
pub async fn set_wake_word_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    {
        let mut config = state.config.lock();
        config.wake_word_enabled = enabled;
        config.save().map_err(|e| e.to_string())?;
    }

    state.voice_manager.set_wake_word_enabled(enabled);

    let mut stream_guard = state.mic_stream.lock();
    if enabled && stream_guard.is_none() {
        match smc_stt::capture::AudioCaptureManager::spawn_mic_stream(state.voice_manager.clone()) {
            Ok(stream) => {
                *stream_guard = Some(stream);
                info!("background microphone stream started for wake-word listening");
            }
            Err(e) => {
                warn!(error = %e, "could not start microphone stream for wake-word");
                return Err(format!("Microphone unavailable: {e}"));
            }
        }
    } else if !enabled && stream_guard.is_some() {
        *stream_guard = None;
        info!("background microphone stream closed");
    }

    Ok(())
}

#[tauri::command]
pub async fn reset_voice_state(state: State<'_, AppState>) -> Result<(), String> {
    state.voice_manager.reset_state();
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
pub struct PillPositionResponse {
    pub position: String,
    pub custom_x: Option<i32>,
    pub custom_y: Option<i32>,
}

#[tauri::command]
pub async fn expand_launcher_window(
    app: AppHandle,
    state: State<'_, AppState>,
    preview_open: Option<bool>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let (target_w, target_h) = if preview_open.unwrap_or(false) {
            (
                crate::hotkey::EXPANDED_WIDTH_PREVIEW,
                crate::hotkey::EXPANDED_HEIGHT + 20,
            )
        } else {
            (
                crate::hotkey::EXPANDED_WIDTH_STANDARD,
                crate::hotkey::EXPANDED_HEIGHT,
            )
        };

        let config = state.config.lock().clone();
        let geoms = crate::hotkey::get_all_monitors(&window);

        let current_pos = window
            .outer_position()
            .map(|p| (p.x, p.y))
            .unwrap_or((0, 0));
        let active_monitor = crate::hotkey::find_monitor_for_point(current_pos, &geoms)
            .or_else(|| {
                window
                    .cursor_position()
                    .ok()
                    .and_then(|c| crate::hotkey::find_monitor_for_cursor((c.x, c.y), &geoms))
            })
            .or_else(|| geoms.first().copied())
            .unwrap_or(crate::hotkey::MonitorGeometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });

        let margin = (
            crate::hotkey::CORNER_MARGIN_X,
            crate::hotkey::CORNER_MARGIN_Y,
        );
        let pill_size = (crate::hotkey::PILL_WIDTH, crate::hotkey::PILL_HEIGHT);
        let custom_pos = match (config.pill_custom_x, config.pill_custom_y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };

        let pill_pos = crate::hotkey::calculate_pill_position(
            &config.pill_position,
            custom_pos,
            &active_monitor,
            pill_size,
            margin,
        );

        let exp_pos = crate::hotkey::calculate_expanded_position(
            pill_pos,
            pill_size,
            (target_w, target_h),
            &config.pill_position,
            &active_monitor,
            margin,
        );

        let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            target_w, target_h,
        )));
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            exp_pos.0, exp_pos.1,
        )));
        let _ = window.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub async fn collapse_launcher_window(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let config = state.config.lock().clone();
        let geoms = crate::hotkey::get_all_monitors(&window);

        let current_pos = window
            .outer_position()
            .map(|p| (p.x, p.y))
            .unwrap_or((0, 0));
        let active_monitor = crate::hotkey::find_monitor_for_point(current_pos, &geoms)
            .or_else(|| geoms.first().copied())
            .unwrap_or(crate::hotkey::MonitorGeometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });

        let margin = (
            crate::hotkey::CORNER_MARGIN_X,
            crate::hotkey::CORNER_MARGIN_Y,
        );
        let pill_size = (crate::hotkey::PILL_WIDTH, crate::hotkey::PILL_HEIGHT);
        let custom_pos = match (config.pill_custom_x, config.pill_custom_y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };

        let pill_pos = crate::hotkey::calculate_pill_position(
            &config.pill_position,
            custom_pos,
            &active_monitor,
            pill_size,
            margin,
        );

        let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            pill_size.0,
            pill_size.1,
        )));
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            pill_pos.0, pill_pos.1,
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn set_pill_position(
    app: AppHandle,
    state: State<'_, AppState>,
    position: String,
) -> Result<(), String> {
    if ![
        "bottom-right",
        "bottom-left",
        "top-right",
        "top-left",
        "custom",
    ]
    .contains(&position.as_str())
    {
        return Err(format!("Invalid pill position: {}", position));
    }

    {
        let mut config = state.config.lock();
        config.pill_position = position;
        config.save().map_err(|e| e.to_string())?;
    }

    let _ = collapse_launcher_window(app, state).await;
    Ok(())
}

#[tauri::command]
pub async fn save_pill_custom_position(
    app: AppHandle,
    state: State<'_, AppState>,
    x: i32,
    y: i32,
) -> Result<(), String> {
    {
        let mut config = state.config.lock();
        config.pill_position = "custom".to_string();
        config.pill_custom_x = Some(x);
        config.pill_custom_y = Some(y);
        config.save().map_err(|e| e.to_string())?;
    }

    let _ = collapse_launcher_window(app, state).await;
    Ok(())
}

#[tauri::command]
pub fn get_pill_position(state: State<'_, AppState>) -> PillPositionResponse {
    let config = state.config.lock();
    PillPositionResponse {
        position: config.pill_position.clone(),
        custom_x: config.pill_custom_x,
        custom_y: config.pill_custom_y,
    }
}
