pub mod config;
pub mod db;
pub mod error;
pub mod governor;
pub mod health;
pub mod jobs;
pub mod migrate;
pub mod project_detector;
pub mod scanner;
pub mod schema;
pub mod watcher;

pub use config::AppConfig;
pub use db::{Database, ProjectRecord};
pub use error::CoreError;
pub use governor::{MockSystemState, ResourceGovernor, SystemStateProvider};
pub use health::{
    MaintenanceStats, ProblemFileRecord, compute_backoff_delay, is_problem_file,
    list_problem_files, record_problem_file, resolve_problem_file, run_sqlite_maintenance,
};
pub use jobs::JobQueue;
pub use project_detector::{
    DetectedProject, detect_project_at, index_projects_in_root, scan_for_projects,
};
pub use scanner::Scanner;
pub use watcher::{FileChangeKind, FileWatcher, PendingChange};
