use crate::state::AppState;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tracing::{error, info, warn};

pub const PILL_WIDTH: u32 = 240;
pub const PILL_HEIGHT: u32 = 44;
pub const EXPANDED_WIDTH_STANDARD: u32 = 680;
pub const EXPANDED_WIDTH_PREVIEW: u32 = 960;
pub const EXPANDED_HEIGHT: u32 = 500;
pub const CORNER_MARGIN_X: i32 = 24;
pub const CORNER_MARGIN_Y: i32 = 24;

/// Register the global hotkey to toggle launcher visibility / expand state.
/// Detects Alt+Space conflicts and records the registration status.
pub fn register_hotkey(app: &AppHandle, state: &AppState) {
    let hotkey_str = state.config.lock().hotkey.clone();

    let shortcut = match hotkey_str.parse::<Shortcut>() {
        Ok(s) => s,
        Err(e) => {
            warn!(hotkey = %hotkey_str, error = %e, "invalid shortcut string, using fallback Ctrl+Space");
            state.hotkey_registered.store(false, Ordering::Relaxed);
            return;
        }
    };

    let hotkey_registered = state.hotkey_registered.clone();

    // Use Tauri's global shortcut plugin.
    let app_handle = app.clone();
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_launcher(&app_handle);
            }
        });

    match result {
        Ok(()) => {
            info!(hotkey = %hotkey_str, "global shortcut registered successfully");
            hotkey_registered.store(true, Ordering::Relaxed);
        }
        Err(e) => {
            error!(
                hotkey = %hotkey_str,
                error = %e,
                "failed to register global shortcut — likely a system conflict (e.g. Windows Alt+Space system menu)"
            );
            hotkey_registered.store(false, Ordering::Relaxed);
        }
    }
}

/// Monitor geometry for window positioning calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl MonitorGeometry {
    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < (self.x + self.width as i32)
            && py >= self.y
            && py < (self.y + self.height as i32)
    }

    pub fn contains_cursor(&self, cx: f64, cy: f64) -> bool {
        cx >= self.x as f64
            && cx < (self.x + self.width as i32) as f64
            && cy >= self.y as f64
            && cy < (self.y + self.height as i32) as f64
    }
}

/// Calculate top-left physical coordinates for the collapsed status pill.
pub fn calculate_pill_position(
    dock: &str,
    custom: Option<(i32, i32)>,
    monitor: &MonitorGeometry,
    pill_size: (u32, u32),
    margin: (i32, i32),
) -> (i32, i32) {
    match dock {
        "bottom-left" => {
            let x = monitor.x + margin.0;
            let y = monitor.y + monitor.height as i32 - pill_size.1 as i32 - margin.1;
            clamp_window_to_monitor((x, y), pill_size, monitor)
        }
        "top-right" => {
            let x = monitor.x + monitor.width as i32 - pill_size.0 as i32 - margin.0;
            let y = monitor.y + margin.1;
            clamp_window_to_monitor((x, y), pill_size, monitor)
        }
        "top-left" => {
            let x = monitor.x + margin.0;
            let y = monitor.y + margin.1;
            clamp_window_to_monitor((x, y), pill_size, monitor)
        }
        "custom" => {
            if let Some((cx, cy)) = custom {
                clamp_window_to_monitor((cx, cy), pill_size, monitor)
            } else {
                // Fallback to bottom-right if no custom coordinates saved
                calculate_pill_position("bottom-right", None, monitor, pill_size, margin)
            }
        }
        _ => {
            // Default "bottom-right"
            let x = monitor.x + monitor.width as i32 - pill_size.0 as i32 - margin.0;
            let y = monitor.y + monitor.height as i32 - pill_size.1 as i32 - margin.1;
            clamp_window_to_monitor((x, y), pill_size, monitor)
        }
    }
}

/// Calculate top-left physical coordinates for the expanded search panel anchored near the pill.
pub fn calculate_expanded_position(
    pill_pos: (i32, i32),
    pill_size: (u32, u32),
    exp_size: (u32, u32),
    dock: &str,
    monitor: &MonitorGeometry,
    margin: (i32, i32),
) -> (i32, i32) {
    match dock {
        "bottom-left" => {
            let x = monitor.x + margin.0;
            let y = monitor.y + monitor.height as i32 - exp_size.1 as i32 - margin.1;
            clamp_window_to_monitor((x, y), exp_size, monitor)
        }
        "top-right" => {
            let x = monitor.x + monitor.width as i32 - exp_size.0 as i32 - margin.0;
            let y = monitor.y + margin.1;
            clamp_window_to_monitor((x, y), exp_size, monitor)
        }
        "top-left" => {
            let x = monitor.x + margin.0;
            let y = monitor.y + margin.1;
            clamp_window_to_monitor((x, y), exp_size, monitor)
        }
        "custom" => {
            // Determine vertical growth direction: if pill is in lower half of monitor, grow upwards.
            let mid_y = monitor.y + (monitor.height as i32 / 2);
            let y = if pill_pos.1 >= mid_y {
                pill_pos.1 + pill_size.1 as i32 - exp_size.1 as i32
            } else {
                pill_pos.1
            };

            // Determine horizontal growth direction: if pill is in right half of monitor, grow leftwards.
            let mid_x = monitor.x + (monitor.width as i32 / 2);
            let x = if pill_pos.0 >= mid_x {
                pill_pos.0 + pill_size.0 as i32 - exp_size.0 as i32
            } else {
                pill_pos.0
            };

            clamp_window_to_monitor((x, y), exp_size, monitor)
        }
        _ => {
            // Default "bottom-right": anchor bottom-right corner
            let x = monitor.x + monitor.width as i32 - exp_size.0 as i32 - margin.0;
            let y = monitor.y + monitor.height as i32 - exp_size.1 as i32 - margin.1;
            clamp_window_to_monitor((x, y), exp_size, monitor)
        }
    }
}

/// Clamp a window rectangle `(pos, size)` within `monitor` boundary.
pub fn clamp_window_to_monitor(
    pos: (i32, i32),
    size: (u32, u32),
    monitor: &MonitorGeometry,
) -> (i32, i32) {
    let min_x = monitor.x;
    let max_x = (monitor.x + monitor.width as i32 - size.0 as i32).max(min_x);
    let min_y = monitor.y;
    let max_y = (monitor.y + monitor.height as i32 - size.1 as i32).max(min_y);

    let clamped_x = pos.0.clamp(min_x, max_x);
    let clamped_y = pos.1.clamp(min_y, max_y);
    (clamped_x, clamped_y)
}

/// Find monitor containing the given point (e.g. window origin or custom pill coordinates).
pub fn find_monitor_for_point(
    point: (i32, i32),
    monitors: &[MonitorGeometry],
) -> Option<MonitorGeometry> {
    monitors
        .iter()
        .find(|m| m.contains_point(point.0, point.1))
        .copied()
}

/// Find monitor containing mouse cursor.
pub fn find_monitor_for_cursor(
    cursor_pos: (f64, f64),
    monitors: &[MonitorGeometry],
) -> Option<MonitorGeometry> {
    monitors
        .iter()
        .find(|m| m.contains_cursor(cursor_pos.0, cursor_pos.1))
        .copied()
}

/// Calculate the top-left (x, y) physical coordinates to center a window of `win_size`
/// on the monitor containing `cursor_pos`. Returns `None` if the cursor is outside all monitors.
pub fn calculate_monitor_center(
    cursor_pos: (f64, f64),
    monitors: &[MonitorGeometry],
    win_size: (u32, u32),
) -> Option<(i32, i32)> {
    if let Some(m) = find_monitor_for_cursor(cursor_pos, monitors) {
        let center_x = m.x + ((m.width as i32 - win_size.0 as i32) / 2);
        let center_y = m.y + ((m.height as i32 - win_size.1 as i32) / 2);
        Some((center_x, center_y))
    } else {
        None
    }
}

/// Extract monitor geometries from Tauri's webview window.
pub fn get_all_monitors(window: &tauri::WebviewWindow) -> Vec<MonitorGeometry> {
    if let Ok(monitors) = window.available_monitors() {
        monitors
            .into_iter()
            .map(|m| {
                let pos = m.position();
                let size = m.size();
                MonitorGeometry {
                    x: pos.x,
                    y: pos.y,
                    width: size.width,
                    height: size.height,
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Center the window on the monitor where the mouse cursor is located, or fallback to center.
pub fn center_on_cursor_monitor(window: &tauri::WebviewWindow) {
    if let Ok(cursor_pos) = window.cursor_position()
        && let Ok(win_size) = window.outer_size()
    {
        let geoms = get_all_monitors(window);
        if let Some((cx, cy)) = calculate_monitor_center(
            (cursor_pos.x, cursor_pos.y),
            &geoms,
            (win_size.width, win_size.height),
        ) {
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                cx, cy,
            )));
            return;
        }
    }
    let _ = window.center();
}

/// Toggle launcher state: if collapsed (pill state) -> expand and focus; if expanded -> collapse to pill.
pub fn toggle_launcher(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(true);
        if !is_visible {
            let _ = window.show();
        }
        let _ = window.set_focus();
        let _ = window.emit("launcher-toggle", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_monitor_cursor_centering() {
        let monitors = vec![MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let win_size = (680, 480);

        // Cursor at center (960, 540)
        let center = calculate_monitor_center((960.0, 540.0), &monitors, win_size);
        assert_eq!(center, Some(((1920 - 680) / 2, (1080 - 480) / 2)));
        assert_eq!(center, Some((620, 300)));

        // Cursor at top-left edge (0, 0)
        let edge_center = calculate_monitor_center((0.0, 0.0), &monitors, win_size);
        assert_eq!(edge_center, Some((620, 300)));
    }

    #[test]
    fn test_multi_monitor_cursor_centering() {
        let monitors = vec![
            MonitorGeometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorGeometry {
                x: 1920,
                y: 0,
                width: 2560,
                height: 1440,
            },
            MonitorGeometry {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        let win_size = (680, 480);

        // Cursor on primary monitor
        let p_center = calculate_monitor_center((500.0, 400.0), &monitors, win_size);
        assert_eq!(p_center, Some((620, 300)));

        // Cursor on right 1440p monitor (x = 2500, y = 700)
        let r_center = calculate_monitor_center((2500.0, 700.0), &monitors, win_size);
        let expected_rx = 1920 + (2560 - 680) / 2;
        let expected_ry = (1440 - 480) / 2;
        assert_eq!(r_center, Some((expected_rx, expected_ry)));

        // Cursor on left monitor (x = -500, y = 300)
        let l_center = calculate_monitor_center((-500.0, 300.0), &monitors, win_size);
        let expected_lx = -1920 + (1920 - 680) / 2;
        let expected_ly = (1080 - 480) / 2;
        assert_eq!(l_center, Some((expected_lx, expected_ly)));
    }

    #[test]
    fn test_pill_position_bottom_right() {
        let monitor = MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pill_size = (PILL_WIDTH, PILL_HEIGHT);
        let margin = (CORNER_MARGIN_X, CORNER_MARGIN_Y);

        let pos = calculate_pill_position("bottom-right", None, &monitor, pill_size, margin);
        assert_eq!(pos, (1920 - 240 - 24, 1080 - 44 - 24));
        assert_eq!(pos, (1656, 1012));
    }

    #[test]
    fn test_pill_position_bottom_left() {
        let monitor = MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pill_size = (PILL_WIDTH, PILL_HEIGHT);
        let margin = (CORNER_MARGIN_X, CORNER_MARGIN_Y);

        let pos = calculate_pill_position("bottom-left", None, &monitor, pill_size, margin);
        assert_eq!(pos, (24, 1080 - 44 - 24));
        assert_eq!(pos, (24, 1012));
    }

    #[test]
    fn test_pill_position_top_right() {
        let monitor = MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pill_size = (PILL_WIDTH, PILL_HEIGHT);
        let margin = (CORNER_MARGIN_X, CORNER_MARGIN_Y);

        let pos = calculate_pill_position("top-right", None, &monitor, pill_size, margin);
        assert_eq!(pos, (1920 - 240 - 24, 24));
        assert_eq!(pos, (1656, 24));
    }

    #[test]
    fn test_pill_position_top_left() {
        let monitor = MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pill_size = (PILL_WIDTH, PILL_HEIGHT);
        let margin = (CORNER_MARGIN_X, CORNER_MARGIN_Y);

        let pos = calculate_pill_position("top-left", None, &monitor, pill_size, margin);
        assert_eq!(pos, (24, 24));
    }

    #[test]
    fn test_pill_position_custom_and_clamping() {
        let monitor = MonitorGeometry {
            x: 100,
            y: 100,
            width: 1920,
            height: 1080,
        };
        let pill_size = (240, 44);
        let margin = (24, 24);

        // Within bounds
        let pos = calculate_pill_position("custom", Some((500, 600)), &monitor, pill_size, margin);
        assert_eq!(pos, (500, 600));

        // Out of bounds right/bottom clamped
        let pos_out =
            calculate_pill_position("custom", Some((3000, 2000)), &monitor, pill_size, margin);
        assert_eq!(pos_out, (100 + 1920 - 240, 100 + 1080 - 44));

        // Out of bounds left/top clamped
        let pos_neg = calculate_pill_position("custom", Some((0, 0)), &monitor, pill_size, margin);
        assert_eq!(pos_neg, (100, 100));
    }

    #[test]
    fn test_expanded_position_growth_directions() {
        let monitor = MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pill_size = (240, 44);
        let exp_size = (680, 500);
        let margin = (24, 24);

        // Bottom-right docked: grows upwards and leftwards
        let pill_br = calculate_pill_position("bottom-right", None, &monitor, pill_size, margin);
        let exp_br = calculate_expanded_position(
            pill_br,
            pill_size,
            exp_size,
            "bottom-right",
            &monitor,
            margin,
        );
        assert_eq!(exp_br, (1920 - 680 - 24, 1080 - 500 - 24));
        assert_eq!(exp_br, (1216, 556));

        // Top-left docked: grows downwards and rightwards
        let pill_tl = calculate_pill_position("top-left", None, &monitor, pill_size, margin);
        let exp_tl =
            calculate_expanded_position(pill_tl, pill_size, exp_size, "top-left", &monitor, margin);
        assert_eq!(exp_tl, (24, 24));
    }

    #[test]
    fn test_cursor_out_of_bounds_fallback() {
        let monitors = vec![MonitorGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let win_size = (680, 480);

        // Cursor at (-100, -100) outside known display
        let out_center = calculate_monitor_center((-100.0, -100.0), &monitors, win_size);
        assert_eq!(out_center, None);
    }
}
