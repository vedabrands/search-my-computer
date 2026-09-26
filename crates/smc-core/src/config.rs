use crate::error::{CoreError, CoreResult};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Default exclusion patterns for sensitive and noisy directories.
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "**/node_modules/**",
    "**/.git/**",
    "**/.svn/**",
    "**/.hg/**",
    "**/target/**",
    "**/__pycache__/**",
    "**/.cache/**",
    "**/Cache/**",
    "**/CacheStorage/**",
    "**/.tmp/**",
    "**/temp/**",
    "**/tmp/**",
    "**/Temp/**",
    "**/$Recycle.Bin/**",
    "**/System Volume Information/**",
    "**/Windows/**",
    "**/Program Files/**",
    "**/Program Files (x86)/**",
    "**/ProgramData/**",
    // Browser profiles — contain sensitive session data.
    "**/AppData/Local/Google/Chrome/**",
    "**/AppData/Local/Microsoft/Edge/**",
    "**/AppData/Roaming/Mozilla/Firefox/**",
    "**/AppData/Local/BraveSoftware/**",
    // Credential stores — never index these.
    "**/.ssh/**",
    "**/.gnupg/**",
    "**/.aws/**",
    "**/.azure/**",
    "**/AppData/Local/1Password/**",
    "**/AppData/Roaming/KeePass/**",
    "**/AppData/Local/LastPass/**",
    // Package manager caches.
    "**/.npm/**",
    "**/.cargo/registry/**",
    "**/.rustup/**",
    "**/.nuget/**",
    "**/.gradle/**",
    "**/.m2/**",
    // IDE caches.
    "**/.vscode/**",
    "**/.idea/**",
    // Virtual environments.
    "**/.venv/**",
    "**/venv/**",
    "**/env/**",
];

/// Maximum file size to index by default (50 MB).
pub const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Default number of indexing threads.
pub const DEFAULT_INDEX_THREADS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Global hotkey to toggle the launcher (e.g. "Alt+Space").
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Theme: "system", "light", or "dark".
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Folders the user has added for indexing.
    #[serde(default)]
    pub indexed_folders: Vec<String>,

    /// Glob patterns to exclude from indexing.
    #[serde(default = "default_exclusions")]
    pub exclusions: Vec<String>,

    /// Maximum file size (bytes) to index.
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,

    /// Number of indexing worker threads.
    #[serde(default = "default_index_threads")]
    pub index_threads: usize,

    /// Whether indexing is paused.
    #[serde(default)]
    pub indexing_paused: bool,

    /// Whether image indexing (OCR, QR, metadata, CLIP) is enabled.
    #[serde(default)]
    pub enable_image_indexing: bool,

    /// Specific folders restricted for image indexing (if empty, applies to all indexed folders).
    #[serde(default)]
    pub image_folders: Vec<String>,

    /// Whether the app is configured to launch automatically on system login.
    #[serde(default)]
    pub launch_at_login: bool,

    /// Whether the first-run onboarding wizard has been completed.
    #[serde(default)]
    pub onboarding_completed: bool,

    /// Whether document formats (PDF, DOCX, TXT, MD, etc.) are indexed.
    #[serde(default = "default_true")]
    pub index_documents: bool,

    /// Whether source code files (Rust, Python, TS, etc.) are indexed.
    #[serde(default = "default_true")]
    pub index_code: bool,

    /// Whether spreadsheet files (XLSX, CSV) are indexed.
    #[serde(default = "default_true")]
    pub index_spreadsheets: bool,

    /// Whether archive formats (ZIP, TAR, etc.) are scanned.
    #[serde(default = "default_true")]
    pub index_archives: bool,

    /// Battery power governor policy ("throttle" or "pause").
    #[serde(default = "default_battery_policy")]
    pub battery_policy: String,

    /// UI language code (e.g. "en").
    #[serde(default = "default_language")]
    pub language: String,

    /// Max RAM allocation limit before unloading model sessions (MB).
    #[serde(default = "default_max_ram_mb")]
    pub max_ram_mb: u64,

    /// Path to an imported .lic license file, if any.
    #[serde(default)]
    pub license_file_path: Option<String>,

    /// Whether always-on wake-word voice detection is enabled (opt-in, off by default).
    #[serde(default)]
    pub wake_word_enabled: bool,

    /// Target wake-word phrase (e.g. "Kira", "hey_jarvis", "alexa").
    #[serde(default = "default_wake_word_phrase")]
    pub wake_word_phrase: String,

    /// Wake-word detection threshold score [0.0, 1.0].
    #[serde(default = "default_wake_word_threshold")]
    pub wake_word_threshold: f32,

    /// Silence timeout (seconds) before auto-stopping voice capture (default 15s).
    #[serde(default = "default_voice_silence_timeout")]
    pub voice_silence_timeout_secs: f32,

    /// Maximum continuous voice capture duration (seconds) (default 45s).
    #[serde(default = "default_voice_max_duration")]
    pub voice_max_duration_secs: f32,

    /// Screen dock position for the floating status pill: "bottom-right", "bottom-left", "top-right", "top-left", "custom".
    #[serde(default = "default_pill_position")]
    pub pill_position: String,

    /// Custom X coordinate when pill_position is "custom".
    #[serde(default)]
    pub pill_custom_x: Option<i32>,

    /// Custom Y coordinate when pill_position is "custom".
    #[serde(default)]
    pub pill_custom_y: Option<i32>,
}

fn default_pill_position() -> String {
    "bottom-right".into()
}

fn default_wake_word_phrase() -> String {
    "Kira".into()
}
fn default_wake_word_threshold() -> f32 {
    0.5
}
fn default_voice_silence_timeout() -> f32 {
    15.0
}
fn default_voice_max_duration() -> f32 {
    45.0
}

fn default_true() -> bool {
    true
}
fn default_battery_policy() -> String {
    "throttle".into()
}
fn default_language() -> String {
    "en".into()
}
fn default_max_ram_mb() -> u64 {
    150
}

fn default_hotkey() -> String {
    "Alt+Space".into()
}
fn default_theme() -> String {
    "system".into()
}
fn default_exclusions() -> Vec<String> {
    DEFAULT_EXCLUSIONS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}
fn default_max_file_size() -> u64 {
    DEFAULT_MAX_FILE_SIZE
}
fn default_index_threads() -> usize {
    DEFAULT_INDEX_THREADS
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            theme: default_theme(),
            indexed_folders: Vec::new(),
            exclusions: default_exclusions(),
            max_file_size: default_max_file_size(),
            index_threads: default_index_threads(),
            indexing_paused: false,
            enable_image_indexing: false,
            image_folders: Vec::new(),
            launch_at_login: false,
            onboarding_completed: false,
            index_documents: true,
            index_code: true,
            index_spreadsheets: true,
            index_archives: true,
            battery_policy: default_battery_policy(),
            language: default_language(),
            max_ram_mb: default_max_ram_mb(),
            license_file_path: None,
            wake_word_enabled: false,
            wake_word_phrase: default_wake_word_phrase(),
            wake_word_threshold: default_wake_word_threshold(),
            voice_silence_timeout_secs: default_voice_silence_timeout(),
            voice_max_duration_secs: default_voice_max_duration(),
            pill_position: default_pill_position(),
            pill_custom_x: None,
            pill_custom_y: None,
        }
    }
}

impl AppConfig {
    /// Load config from the OS app-data directory. Returns defaults if the
    /// file doesn't exist or is invalid.
    pub fn load() -> CoreResult<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            info!("no config file found, using defaults");
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)?;

        match serde_json::from_str::<Self>(&contents) {
            Ok(config) => {
                info!(?path, "config loaded");
                Ok(config)
            }
            Err(e) => {
                warn!(?path, %e, "config file invalid, using defaults");
                Ok(Self::default())
            }
        }
    }

    /// Load config from a specific path.
    pub fn load_from(path: &Path) -> CoreResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        match serde_json::from_str::<Self>(&contents) {
            Ok(config) => Ok(config),
            Err(e) => {
                warn!(?path, %e, "config file invalid, using defaults");
                Ok(Self::default())
            }
        }
    }

    /// Save the current config to the OS app-data directory.
    pub fn save(&self) -> CoreResult<()> {
        let path = Self::config_path()?;
        self.save_to(&path)
    }

    /// Save the current config to a specific path.
    pub fn save_to(&self, path: &Path) -> CoreResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        info!(?path, "config saved");
        Ok(())
    }

    /// Validate the config, returning a list of issues.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if self.hotkey.is_empty() {
            issues.push("hotkey must not be empty".into());
        }

        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            issues.push(format!(
                "invalid theme '{}', must be system/light/dark",
                self.theme
            ));
        }

        if self.index_threads == 0 || self.index_threads > 8 {
            issues.push(format!(
                "index_threads must be 1-8, got {}",
                self.index_threads
            ));
        }

        if self.max_file_size == 0 {
            issues.push("max_file_size must be > 0".into());
        }

        if !["throttle", "pause"].contains(&self.battery_policy.as_str()) {
            issues.push(format!(
                "invalid battery_policy '{}', must be throttle/pause",
                self.battery_policy
            ));
        }

        if self.max_ram_mb == 0 {
            issues.push("max_ram_mb must be > 0".into());
        }

        if ![
            "bottom-right",
            "bottom-left",
            "top-right",
            "top-left",
            "custom",
        ]
        .contains(&self.pill_position.as_str())
        {
            issues.push(format!(
                "invalid pill_position '{}', must be bottom-right/bottom-left/top-right/top-left/custom",
                self.pill_position
            ));
        }

        issues
    }

    /// Returns the path to the config file in the OS app-data directory.
    pub fn config_path() -> CoreResult<PathBuf> {
        let dirs = ProjectDirs::from("com", "searchmycomputer", "SearchMyComputer")
            .ok_or_else(|| CoreError::Config("cannot determine app data directory".into()))?;
        Ok(dirs.config_dir().join("config.json"))
    }

    /// Returns the path to the data directory (for DB, logs, etc.).
    pub fn data_dir() -> CoreResult<PathBuf> {
        let dirs = ProjectDirs::from("com", "searchmycomputer", "SearchMyComputer")
            .ok_or_else(|| CoreError::Config("cannot determine app data directory".into()))?;
        Ok(dirs.data_dir().to_path_buf())
    }

    /// Returns the path to the database file.
    pub fn db_path() -> CoreResult<PathBuf> {
        Ok(Self::data_dir()?.join("index.db"))
    }

    /// Returns the path to the log directory.
    pub fn log_dir() -> CoreResult<PathBuf> {
        Ok(Self::data_dir()?.join("logs"))
    }

    /// Configure launch at login in the operating system.
    #[cfg(windows)]
    pub fn set_launch_at_login(enable: bool) -> CoreResult<()> {
        use std::process::Command;
        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy().to_string();
        if enable {
            let val = format!("\"{}\"", exe_str);
            let _ = Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "SearchMyComputer",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &val,
                    "/f",
                ])
                .output();
        } else {
            let _ = Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "SearchMyComputer",
                    "/f",
                ])
                .output();
        }
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn set_launch_at_login(_enable: bool) -> CoreResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.hotkey, "Alt+Space");
        assert_eq!(config.theme, "system");
        assert!(config.indexed_folders.is_empty());
        assert!(!config.exclusions.is_empty());
        assert_eq!(config.max_file_size, DEFAULT_MAX_FILE_SIZE);
        assert_eq!(config.index_threads, 2);
        assert!(!config.indexing_paused);
    }

    #[test]
    fn test_save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");

        let config = AppConfig {
            hotkey: "Ctrl+Space".into(),
            indexed_folders: vec!["C:\\Users\\test\\Documents".into()],
            ..Default::default()
        };
        config.save_to(&path).unwrap();

        let loaded = AppConfig::load_from(&path).unwrap();
        assert_eq!(loaded.hotkey, "Ctrl+Space");
        assert_eq!(loaded.indexed_folders.len(), 1);
    }

    #[test]
    fn test_validation() {
        let mut config = AppConfig::default();
        assert!(config.validate().is_empty());

        config.theme = "purple".into();
        config.index_threads = 0;
        let issues = config.validate();
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn test_load_invalid_json_returns_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.json");
        std::fs::write(&path, "not json at all").unwrap();

        let config = AppConfig::load_from(&path).unwrap();
        assert_eq!(config.hotkey, "Alt+Space");
    }
}
