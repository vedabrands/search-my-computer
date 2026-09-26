pub mod commands;
pub mod hotkey;
pub mod logging;
pub mod state;
pub mod tray;

use commands::*;
use hotkey::register_hotkey;
use smc_core::config::AppConfig;
use smc_core::db::Database;
use state::AppState;
use tauri::{Emitter, Manager};
use tracing::{error, info};

pub fn run() {
    // 1. Load config.
    let config = AppConfig::load().unwrap_or_default();

    // 2. Initialize logging.
    let log_dir = AppConfig::log_dir().unwrap_or_else(|_| std::path::PathBuf::from("logs"));
    logging::init_logging(log_dir);
    info!("SearchMyComputer starting up");

    // 3. Open database.
    let db_path = AppConfig::db_path().unwrap_or_else(|_| std::path::PathBuf::from("index.db"));
    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            error!(error = %e, "failed to open database, falling back to in-memory");
            Database::open_in_memory().expect("in-memory db must succeed")
        }
    };

    let app_state = AppState::new(db, config);

    // 4. Recover crashed jobs and start workers.
    let _ = app_state.job_queue.recover();
    let registry = app_state.extractor_registry.clone();
    let limits = app_state.extraction_limits.clone();
    let embedder = app_state.embedder.clone();
    let vector_index = app_state.vector_index.clone();
    let vision_pipeline = app_state.vision_pipeline.clone();
    let config_arc = app_state.config.clone();

    app_state.job_queue.start_workers(move |job, db| {
        match job.kind.as_str() {
            smc_core::schema::job_kind::INDEX_FILENAME => {
                if let Some(file_id) = job.file_id {
                    let conn = db.writer();
                    let now = chrono::Utc::now().to_rfc3339();

                    let file_info: Option<(String, String)> = {
                        let reader = db.reader().ok();
                        reader.and_then(|r| {
                            r.query_row(
                                "SELECT path, kind FROM files WHERE id = ?1",
                                rusqlite::params![file_id],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .ok()
                        })
                    };

                    if let Some((path_str, kind)) = file_info {
                        if kind == smc_core::schema::file_kind::IMAGE {
                            let config = config_arc.lock();
                            let image_enabled = config.enable_image_indexing;
                            let image_folders = config.image_folders.clone();
                            drop(config);

                            let allowed = if image_folders.is_empty() {
                                true
                            } else {
                                let norm_path = path_str.replace('\\', "/");
                                image_folders.iter().any(|folder| {
                                    let norm_folder = folder.replace('\\', "/");
                                    norm_path.starts_with(&norm_folder)
                                })
                            };

                            if image_enabled && allowed {
                                let _ = conn.execute(
                                    "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                                     SELECT ?1, ?2, 1, ?3, ?4
                                     WHERE NOT EXISTS (
                                         SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                                     )",
                                    rusqlite::params![
                                        smc_core::schema::job_kind::VISION,
                                        file_id,
                                        smc_core::schema::job_state::PENDING,
                                        now
                                    ],
                                );
                            }
                        } else {
                            let _ = conn.execute(
                                "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
                                 SELECT ?1, ?2, 5, ?3, ?4
                                 WHERE NOT EXISTS (
                                     SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
                                 )",
                                rusqlite::params![
                                    smc_core::schema::job_kind::EXTRACT,
                                    file_id,
                                    smc_core::schema::job_state::PENDING,
                                    now
                                ],
                            );
                        }
                    }
                }
                Ok(())
            }
            smc_core::schema::job_kind::EXTRACT => {
                if let Some(file_id) = job.file_id {
                    smc_extract::process_extract_job(db, file_id, &registry, &limits)
                } else {
                    Ok(())
                }
            }
            smc_core::schema::job_kind::VISION => {
                if let Some(file_id) = job.file_id {
                    let config = config_arc.lock();
                    let image_enabled = config.enable_image_indexing;
                    drop(config);

                    if image_enabled {
                        smc_vision::process_vision_job(db, file_id, vision_pipeline.as_ref())
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            smc_core::schema::job_kind::EMBED => {
                if let Some(file_id) = job.file_id {
                    if let Some(ref emb) = embedder {
                        smc_embed::process_file_embedding(
                            db,
                            file_id,
                            emb.as_ref(),
                            vector_index.as_ref(),
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                    } else {
                        // Graceful degradation: no embedder available, ignore job.
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    });

    // 5. Start background file watcher on configured folders.
    {
        let folders = {
            let cfg = app_state.config.lock();
            cfg.indexed_folders.clone()
        };
        let watcher_arc = app_state.watcher.clone();
        std::thread::spawn(move || {
            let mut watcher = watcher_arc.lock();
            let _ = watcher.reconcile_startup(&folders);
            let _ = watcher.start(&folders);
        });
    }

    // 6. Spawn periodic background SQLite maintenance loop (runs every 10 min when idle & on AC power).
    {
        let db_clone = app_state.db.clone();
        let governor_clone = app_state.governor.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(600));
                if governor_clone.is_maintenance_allowed() {
                    info!("running background SQLite maintenance (idle & plugged in)");
                    let _ = smc_core::health::run_sqlite_maintenance(&db_clone);
                }
            }
        });
    }

    // 7. Build Tauri application.
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            search,
            semantic_search,
            parse_nlq,
            search_projects,
            reveal_in_folder,
            open_terminal_in_folder,
            add_folder,
            remove_folder,
            start_scan,
            get_index_status,
            get_config,
            update_config,
            pause_indexing,
            resume_indexing,
            get_governor_status,
            get_problem_files,
            set_launch_at_login,
            hide_window,
            get_image_details,
            detect_system_folders,
            test_exclusion_pattern,
            get_file_preview,
            delete_all_data,
            rebuild_index,
            export_diagnostics,
            clear_problem_files,
            retry_problem_files,
            get_license_status,
            import_license,
            check_for_updates,
            start_voice_capture,
            stop_voice_capture,
            get_voice_state,
            pause_wake_word,
            resume_wake_word,
            set_wake_word_enabled,
            reset_voice_state,
            expand_launcher_window,
            collapse_launcher_window,
            set_pill_position,
            save_pill_custom_position,
            get_pill_position,
        ])
        .setup(|app| {
            let handle = app.handle();
            let state = handle.state::<AppState>();

            // Setup system tray.
            if let Err(e) = tray::create_tray(handle) {
                error!(error = %e, "failed to create system tray");
            }

            // Register global hotkey.
            register_hotkey(handle, &state);

            // Register voice callbacks
            let handle_wake = handle.clone();
            state.voice_manager.set_on_wake_word(move || {
                info!("wake word event triggered, focusing window and emitting event");
                if let Some(window) = handle_wake.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.emit("voice:wake_word", ());
                }
            });

            let handle_trans = handle.clone();
            state.voice_manager.set_on_transcription(move |text| {
                info!("transcription event completed, emitting event to webview");
                if let Some(window) = handle_trans.get_webview_window("main") {
                    let _ = window.emit("voice:transcription", text);
                }
            });

            // Initial status pill positioning.
            if let Some(window) = handle.get_webview_window("main") {
                let config = state.config.lock().clone();
                let geoms = hotkey::get_all_monitors(&window);
                let active_mon = geoms.first().copied().unwrap_or(hotkey::MonitorGeometry {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                });
                let margin = (hotkey::CORNER_MARGIN_X, hotkey::CORNER_MARGIN_Y);
                let pill_size = (hotkey::PILL_WIDTH, hotkey::PILL_HEIGHT);
                let custom = match (config.pill_custom_x, config.pill_custom_y) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                };
                let initial_pos = hotkey::calculate_pill_position(
                    &config.pill_position,
                    custom,
                    &active_mon,
                    pill_size,
                    margin,
                );
                let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
                    pill_size.0,
                    pill_size.1,
                )));
                let _ = window.set_position(tauri::Position::Physical(
                    tauri::PhysicalPosition::new(initial_pos.0, initial_pos.1),
                ));
                let _ = window.show();

                // Collapse launcher panel on blur back to status pill.
                let handle_blur = handle.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = handle_blur.emit("launcher-collapse", ());
                    }
                });
            }

            info!("Tauri setup complete");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
