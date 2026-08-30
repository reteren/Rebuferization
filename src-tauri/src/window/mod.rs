//! Popup placement, backdrop, focus handling, and paste injection.
//!
//! OWNER: worker W3.

pub mod paste;
pub mod position;
pub mod vibrancy;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::error::{AppError, AppResult};
use crate::AppState;

pub const POPUP_LABEL: &str = "popup";
pub const SETTINGS_LABEL: &str = "settings";

/// The outside-click dismissal handler is registered once at startup.
static DISMISS_WIRED: AtomicBool = AtomicBool::new(false);

/// Serializes show decisions. `show_popup` runs on both the hotkey thread and
/// the main thread (tray, second instance); without a lock two concurrent
/// calls both read `is_visible` before either shows, and the second can hide
/// what the first just showed. Only `show_popup` ever holds it for real:
/// blocking callers must not acquire it, because the hotkey thread holds it
/// across `win.show()`/`set_focus()` and would deadlock against a main thread
/// that blocks on the lock instead of servicing the window messages. The
/// dismissal path therefore uses `try_lock` and gives up when a show is in
/// flight (the show reasserts focus anyway).
static SHOW_LOCK: Mutex<()> = Mutex::new(());

/// Caches the foreground `HWND`, positions the popup at the cursor clamped to
/// the work area, applies the backdrop, and shows it.
pub fn show_popup(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(POPUP_LABEL)
        .ok_or_else(|| AppError::Other("popup window not found".into()))?;

    let _guard = SHOW_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Win+V toggles: a press while visible closes the popup. Returning early
    // also keeps the cache from being overwritten with the popup's own HWND.
    if win.is_visible().map_err(tauri_err)? {
        return hide_popup_impl(app, true);
    }

    // Cache the paste target BEFORE the popup takes focus.
    // FFI: GetForegroundWindow takes no arguments; a NULL result is checked.
    let foreground = unsafe { GetForegroundWindow() };
    if !foreground.is_invalid() && !is_own_window(app, foreground) {
        paste::set_target(foreground);
    } else {
        // Never cache our own windows as a paste target, and never keep a
        // stale one: a paste into the popup or the settings window is the
        // destructive case SPEC 5.4 warns about. A cleared cache means the
        // next paste is skipped with a log line.
        paste::clear_target();
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

/// Hides the popup and restores the previously cached foreground window. Used
/// by the command paths (Esc, close-on-copy, paste), where we are the reason
/// focus left and the previous window should get it back.
pub fn hide_popup(app: &AppHandle) -> AppResult<()> {
    hide_popup_impl(app, true)
}

/// Dismissal path for when the user has already given focus to another
/// window (outside click): that window keeps focus, so nothing is restored.
pub fn hide_popup_dismissed(app: &AppHandle) -> AppResult<()> {
    // Never block on the lock: see the SHOW_LOCK comment — a blocking
    // acquisition here can deadlock against a show in flight from the hotkey
    // thread. When a show holds the lock, the popup is being (re)shown and
    // will reassert focus itself.
    if let Ok(_guard) = SHOW_LOCK.try_lock() {
        hide_popup_impl(app, false)
    } else {
        Ok(())
    }
}

fn hide_popup_impl(app: &AppHandle, restore: bool) -> AppResult<()> {
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
    if was_visible && restore {
        paste::restore_foreground_window();
    }
    Ok(())
}

/// True when `hwnd` is one of our own windows (the popup or the settings
/// window), which must never be treated as a paste target.
fn is_own_window(app: &AppHandle, hwnd: HWND) -> bool {
    [POPUP_LABEL, SETTINGS_LABEL].iter().any(|label| {
        app.get_webview_window(label)
            .and_then(|w| w.hwnd().ok())
            .is_some_and(|own| own == hwnd)
    })
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
            // observes as focus loss — that is the dismissal signal. That
            // window already has focus, so the hide must not restore the
            // cached foreground (that would yank focus away from the window
            // the user just clicked).
            if let WindowEvent::Focused(false) = event {
                let _ = hide_popup_dismissed(&app);
            }
        });
    }
    vibrancy::apply(window)
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}