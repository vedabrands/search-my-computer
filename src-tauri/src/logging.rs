use std::path::PathBuf;
use tracing_appender::rolling;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// Set up structured logging with daily rotating log files in the app data dir.
/// Filters out file contents and query text at info level.
pub fn init_logging(log_dir: PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("failed to create log dir: {e}");
        return;
    }

    // Daily rotating file appender.
    let file_appender = rolling::daily(log_dir, "smc.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Keep the guard alive by leaking it (logging runs for the lifetime of the process).
    std::mem::forget(_guard);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,smc_core=debug,smc_search=debug,search_my_computer=debug")
    });

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    let stdout_layer = fmt::layer().with_ansi(true).with_target(false);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();
}
