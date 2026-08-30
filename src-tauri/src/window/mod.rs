//! Popup placement, backdrop, focus handling, and paste injection.
//!
//! OWNER: worker W3.

pub mod paste;
pub mod position;
pub mod vibrancy;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::error::{AppError, AppResult};
use crate::AppState;

pub const POPUP_LABEL: &str = "popup";
pub const SETTINGS_LABEL: &str = "settings";

/// The outside-click dismissal handler is registered once at startup.
static DISMISS_WIRED: AtomicBool = AtomicBool::new(false);

/// Caches the foreground `HWND`, positions the popup at the cursor clamped to
/// the work area, applies the backdrop, and shows it.
pub fn show_popup(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(POPUP_LABEL)
        .ok_or_else(|| AppError::Other("popup window not found".into()))?;

    // Win+V toggles: a press while visible closes the popup. Returning early
    // also keeps the cache from being overwritten with the popup's own HWND.
    if win.is_visible().map_err(tauri_err)? {
        return hide_popup(app);
    }

    // Cache the paste target BEFORE the popup takes focus.
    // FFI: GetForegroundWindow takes no arguments; a NULL result is checked.
    let foreground = unsafe { GetForegroundWindow() };
    if !foreground.is_invalid() {
        paste::set_target(foreground);
    }

    let settings = app.state::<AppState>().settings.get();
    position::place_popup(&win, &settings.window)?;
    win.show().map_err(tauri_err)?;
    // SPEC §11 decision (see docs/DECISIONS.md): the popup activates so the
    // WebView gets plain keyboard input; the cached foreground window is
    // restored on hide.
    win.set_focus().map_err(tauri_err)?;
    Ok(())
}

/// Hides the popup and restores the previously cached foreground window.
pub fn hide_popup(app: &AppHandle) -> AppResult<()> {
    let was_visible = match app.get_webview_window(POPUP_LABEL) {
        Some(win) => {
            let visible = win.is_visible().map_err(tauri_err)?;
            if visible {
                win.hide().map_err(tauri_err)?;
            }
            visible
        }
        None => false,
    };
    if was_visible {
        paste::restore_foreground_window();
    }
    Ok(())
}

/// Centers the settings window on the cursor's monitor and shows it.
pub fn show_settings(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| AppError::Other("settings window not found".into()))?;
    position::center_on_cursor_monitor(&win)?;
    win.show().map_err(tauri_err)?;
    win.set_focus().map_err(tauri_err)?;
    Ok(())
}

/// Applies acrylic on Windows 11, falling back to a flat background on
/// Windows 10. Called once per window at startup. Also wires the popup's
/// outside-click dismissal, which needs to happen before it can be shown.
pub fn apply_backdrop(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == POPUP_LABEL && !DISMISS_WIRED.swap(true, Ordering::SeqCst) {
        let app = window.app_handle().clone();
        window.on_window_event(move |event| {
            // Clicking outside activates another window, which the popup
            // observes as focus loss — that is the dismissal signal.
            if let WindowEvent::Focused(false) = event {
                let _ = hide_popup(&app);
            }
        });
    }
    vibrancy::apply(window)
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}