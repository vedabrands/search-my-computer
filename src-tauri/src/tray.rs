use crate::hotkey::toggle_launcher;
use crate::state::AppState;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tracing::info;

/// Set up the system tray icon and menu.
pub fn create_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let status_item = MenuItem::with_id(app, "status", "Status: Idle", false, None::<&str>)?;
    let open_item = MenuItem::with_id(app, "open", "Open Launcher", true, None::<&str>)?;
    let pause_15_item = MenuItem::with_id(
        app,
        "pause_15",
        "Pause Indexing (15 min)",
        true,
        None::<&str>,
    )?;
    let pause_1h_item = MenuItem::with_id(
        app,
        "pause_1h",
        "Pause Indexing (1 hour)",
        true,
        None::<&str>,
    )?;
    let pause_restart_item = MenuItem::with_id(
        app,
        "pause_restart",
        "Pause Indexing until restart",
        true,
        None::<&str>,
    )?;
    let resume_item = MenuItem::with_id(app, "resume", "Resume Indexing", true, None::<&str>)?;
    let pause_ww_15_item = MenuItem::with_id(
        app,
        "pause_ww_15",
        "Pause Wake Word (15 min)",
        true,
        None::<&str>,
    )?;
    let pause_ww_1h_item = MenuItem::with_id(
        app,
        "pause_ww_1h",
        "Pause Wake Word (1 hour)",
        true,
        None::<&str>,
    )?;
    let resume_ww_item =
        MenuItem::with_id(app, "resume_ww", "Resume Wake Word", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &open_item,
            &pause_15_item,
            &pause_1h_item,
            &pause_restart_item,
            &resume_item,
            &pause_ww_15_item,
            &pause_ww_1h_item,
            &resume_ww_item,
            &settings_item,
            &quit_item,
        ],
    )?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                toggle_launcher(app);
            }
            "pause_15" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.governor.pause_for(Duration::from_secs(15 * 60));
                }
            }
            "pause_1h" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.governor.pause_for(Duration::from_secs(60 * 60));
                }
            }
            "pause_restart" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.governor.pause_until_restart();
                }
            }
            "resume" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.governor.resume();
                }
            }
            "pause_ww_15" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state
                        .voice_manager
                        .pause_wake_word(Duration::from_secs(15 * 60));
                }
            }
            "pause_ww_1h" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state
                        .voice_manager
                        .pause_wake_word(Duration::from_secs(60 * 60));
                }
            }
            "resume_ww" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.voice_manager.resume_wake_word();
                }
            }
            "settings" => {
                toggle_launcher(app);
            }
            "quit" => {
                info!("quit requested from tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_launcher(tray.app_handle());
            }
        })
        .build(app)?;

    // Spawn background tray status updater
    let app_handle = app.clone();
    let status_item_handle = status_item.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            if let Some(state) = app_handle.try_state::<AppState>() {
                let pending = state.job_queue.pending_count().unwrap_or(0) as usize;
                let label = state.governor.status_label(pending);
                let _ = status_item_handle.set_text(format!("Status: {label}"));
            }
        }
    });

    info!("system tray initialized with live status and pause controls");
    Ok(())
}
