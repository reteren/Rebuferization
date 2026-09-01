//! Popup placement, backdrop, focus handling, and paste injection.
//!
//! OWNER: worker W3.

pub mod paste;
pub mod position;
pub mod vibrancy;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, SetForegroundWindow,
    SetWindowLongPtrW, GWL_EXSTYLE, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCLBUTTONDOWN,
    WM_NCLBUTTONUP, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use crate::error::{AppError, AppResult};
use crate::AppState;

pub const POPUP_LABEL: &str = "popup";
pub const SETTINGS_LABEL: &str = "settings";
pub const TRAYMENU_LABEL: &str = "traymenu";

/// True while the popup is inside Windows' modal move/size loop (a resize
/// drag). Dragging a window's edge can transiently deactivate it, and the
/// outside-click dismissal must not treat that as a dismissal — the window
/// has to stay open for the whole drag. Set/cleared from a window subclass
/// (see `wire_popup_subclass`), because tao's `WindowEvent` does not expose
/// `WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE`.
static IN_MOVE_OR_RESIZE: AtomicBool = AtomicBool::new(false);

/// True while the left button is down in the popup's non-client area (the
/// resize border). Pressing the border deactivates the popup *before* the
/// modal loop starts — observed as `WM_NCACTIVATE(0)`/`WM_ACTIVATE(0)` then
/// `Focused(false)` — so the dismissal must also be suppressed from the
/// button-down, not just while the modal loop is running. Cleared on
/// button-up and on `WM_EXITSIZEMOVE`.
static NC_BUTTON_DOWN: AtomicBool = AtomicBool::new(false);

/// The `AppHandle` used by the subclass proc, which cannot capture. Set once
/// when the popup subclass is wired; used to persist the dragged size and to
/// re-focus the popup when a drag ends.
static POPUP_APP: OnceLock<AppHandle> = OnceLock::new();

/// Installs a subclass on the popup's window procedure. The subclass chains
/// to tao's proc (comctl32 subclasses stack), and only observes the enter/
/// exit of the move/size modal loop. `SetWindowSubclass` is safe to call
/// from the thread that owns the window; `apply_backdrop` runs there at
/// startup, before the popup can be shown.
fn wire_popup_subclass(window: &WebviewWindow) -> AppResult<()> {
    let hwnd = window.hwnd().map_err(tauri_err)?;
    let _ = POPUP_APP.set(window.app_handle().clone());
    // FFI: hwnd is the live popup window; the subclass chains to tao's own
    // subclass, which remains the final handler for every message.
    let ok = unsafe { SetWindowSubclass(hwnd, Some(popup_subclass_proc), 0, 0) };
    if !ok.as_bool() {
        tracing::warn!("SetWindowSubclass failed; the popup resize dismissal guard is disabled");
    }
    Ok(())
}

/// Observes the start/end of a modal move/size drag. On drag end the popup is
/// re-focused (the drag can leave it deactivated) and its final size is
/// persisted, so a manually resized popup keeps that size on the next show.
unsafe extern "system" fn popup_subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    match umsg {
        WM_NCLBUTTONDOWN => {
            // Any press in the non-client area (the resize border) is an
            // interaction with the popup itself, never an outside click. It
            // deactivates the popup, so from this moment on the dismissal is
            // suppressed until the button comes back up.
            NC_BUTTON_DOWN.store(true, Ordering::SeqCst);
        }
        WM_NCLBUTTONUP => {
            NC_BUTTON_DOWN.store(false, Ordering::SeqCst);
            refocus_after_nc_interaction(hwnd);
        }
        WM_ENTERSIZEMOVE => {
            IN_MOVE_OR_RESIZE.store(true, Ordering::SeqCst);
        }
        WM_EXITSIZEMOVE => {
            IN_MOVE_OR_RESIZE.store(false, Ordering::SeqCst);
            NC_BUTTON_DOWN.store(false, Ordering::SeqCst);
            if let Some(app) = POPUP_APP.get() {
                persist_resized_size(app.clone());
            }
            // Re-activate the popup so a later genuine focus loss is still
            // observable as the dismissal signal.
            let _ = SetForegroundWindow(hwnd);
        }
        _ => {}
    }
    // FFI: every message is forwarded unchanged to tao's window proc; the
    // subclass only observes, never alters the window's message handling.
    DefSubclassProc(hwnd, umsg, wparam, lparam)
}

/// The popup's non-client interaction (a border click that did not turn into
/// a drag) leaves it deactivated; bring it back so the dismissal still works
/// on the next genuine focus loss.
fn refocus_after_nc_interaction(hwnd: HWND) {
    // FFI: hwnd is the live popup window; SetForegroundWindow is a plain
    // single-window call.
    let _ = unsafe { SetForegroundWindow(hwnd) };
}

/// Writes the popup's current logical size into settings (`window.fixed`)
/// and switches `sizeMode` to `"fixed"`, so the size the user dragged to is
/// what the next `show_popup` restores. Runs on a background thread: the
/// settings write must not block the message loop during `WM_EXITSIZEMOVE`.
fn persist_resized_size(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window(POPUP_LABEL) else {
            tracing::warn!("persist_resized_size: popup window not found");
            return;
        };
        let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
            tracing::warn!("persist_resized_size: could not read window size/scale");
            return;
        };
        if scale <= 0.0 {
            return;
        }
        // As u32, not f64: `round()` yields a float, and serde_json then
        // rejects the patch with "invalid type: floating point 546.0, expected
        // u32". Every resize was silently discarded that way.
        let width = (size.width as f64 / scale).round().max(1.0) as u32;
        let height = (size.height as f64 / scale).round().max(1.0) as u32;
        let Some(state) = app.try_state::<AppState>() else {
            tracing::warn!("persist_resized_size: AppState not available");
            return;
        };
        match state.settings.patch(serde_json::json!({
            "window": {
                "sizeMode": "fixed",
                "fixed": { "width": width, "height": height }
            }
        })) {
            Ok(_) => tracing::info!("persisted resized popup: {width}x{height} (sizeMode=fixed)"),
            Err(e) => tracing::warn!("persist_resized_size: settings patch failed: {e}"),
        }
    });
}

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
    // A drag or border press can never be in flight across a show; clear any
    // stale markers so a later genuine focus loss is always treated as a
    // dismissal.
    IN_MOVE_OR_RESIZE.store(false, Ordering::SeqCst);
    NC_BUTTON_DOWN.store(false, Ordering::SeqCst);

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
/// Shows the tray menu at the cursor. It is our own window rather than a
/// native one because a native HMENU cannot be themed: Windows paints it, and
/// no amount of CSS reaches it. The cost is that dismissal, sizing and
/// placement are ours to handle.
pub fn show_tray_menu(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(TRAYMENU_LABEL)
        .ok_or_else(|| AppError::Other("tray menu window missing".into()))?;
    position::place_at_cursor(&win)?;
    win.show().map_err(tauri_err)?;
    win.set_focus().map_err(tauri_err)?;
    Ok(())
}

pub fn hide_tray_menu(app: &AppHandle) -> AppResult<()> {
    if let Some(win) = app.get_webview_window(TRAYMENU_LABEL) {
        win.hide().map_err(tauri_err)?;
    }
    Ok(())
}

pub fn show_settings(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(SETTINGS_LABEL)
        .ok_or_else(|| AppError::Other("settings window not found".into()))?;
    position::center_on_cursor_monitor(&win)?;
    win.show().map_err(tauri_err)?;
    // After the show, not before: showing the window puts WS_EX_APPWINDOW back,
    // so setting the style once at startup was undone every time.
    //
    // Tauri's own set_skip_taskbar is deliberately not used. It re-shows the
    // window to apply the change, and the deactivation that causes was read by
    // the dismissal below as the user clicking away — the settings window shut
    // itself the moment it opened.
    keep_out_of_taskbar(&win);
    *SETTINGS_SHOWN_AT.lock().unwrap() = Some(Instant::now());
    win.set_focus().map_err(tauri_err)?;
    Ok(())
}

/// True when the window that now holds focus belongs to this process.
///
/// A native folder picker or save dialog is opened by us and runs in our own
/// process, so it takes focus while the user is still working inside settings.
/// Without this check the dismissal would close the settings window the moment
/// "Choose folder" was pressed, and the dialog would be left orphaned.
fn foreground_is_ours() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid != 0 && pid == unsafe { GetCurrentProcessId() }
}

/// Wired once, at startup. Two things the settings window got wrong:
///
/// Closing it destroyed the webview, and every later "open settings" then
/// looked up a window that no longer existed and failed — settings could not
/// be reopened at all until the app was restarted. Hiding instead keeps the
/// window alive, which is also what makes reopening instant.
///
/// And it stayed open behind whatever the user switched to. Losing focus is
/// the dismissal signal, exactly as it is for the popup, except that our own
/// file dialogs must not count as losing it.
static SETTINGS_WIRED: AtomicBool = AtomicBool::new(false);

/// When the settings window was last shown. Focus settles over a few frames —
/// the popup is still hiding, the shell is still handing activation over — and
/// a deactivation seen in that gap is not the user clicking away.
static SETTINGS_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

const SETTINGS_FOCUS_GRACE: Duration = Duration::from_millis(600);

fn settling_after_show() -> bool {
    matches!(*SETTINGS_SHOWN_AT.lock().unwrap(), Some(t) if t.elapsed() < SETTINGS_FOCUS_GRACE)
}

/// Keeps the settings window out of the taskbar.
///
/// `skipTaskbar` in tauri.conf.json is not enough on its own: tao asks the
/// shell to drop the button, which leaves `WS_EX_APPWINDOW` in place, and the
/// button was measurably still there. A tool window never gets one. It is
/// applied while the window is still hidden, which is when the style is free
/// to change; on a visible window Windows would need it re-shown to take.
fn keep_out_of_taskbar(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        tracing::warn!("settings window has no HWND; it may show a taskbar button");
        return;
    };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = (ex & !(WS_EX_APPWINDOW.0 as isize)) | WS_EX_TOOLWINDOW.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
    }
}

fn wire_settings_dismissal(window: &WebviewWindow) {
    if SETTINGS_WIRED.swap(true, Ordering::SeqCst) {
        return;
    }
    let win = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = win.hide();
        }
        WindowEvent::Focused(false) if !foreground_is_ours() && !settling_after_show() => {
            let _ = win.hide();
        }
        _ => {}
    });
}

/// Applies acrylic on Windows 11, falling back to a flat background on
/// Windows 10. Called once per window at startup. Also wires the popup's
/// outside-click dismissal, which needs to happen before it can be shown.
pub fn apply_backdrop(window: &WebviewWindow) -> AppResult<()> {
    if window.label() == SETTINGS_LABEL {
        keep_out_of_taskbar(window);
        wire_settings_dismissal(window);
    }
    if window.label() == POPUP_LABEL {
        wire_popup_subclass(window)?;
        if !DISMISS_WIRED.swap(true, Ordering::SeqCst) {
            let app = window.app_handle().clone();
            window.on_window_event(move |event| {
                // Clicking outside activates another window, which the popup
                // observes as focus loss — that is the dismissal signal. That
                // window already has focus, so the hide must not restore the
                // cached foreground (that would yank focus away from the window
                // the user just clicked).
                //
                // A resize/move drag can also transiently deactivate the popup;
                // that is not a dismissal. The window subclass tracks the modal
                // move/size loop, and while it is active the popup stays open
                // for the whole drag.
                if let WindowEvent::Focused(false) = event {
                    // A press on the resize border or a move/size drag can
                    // transiently deactivate the popup; that is not a
                    // dismissal. The window subclass tracks both the
                    // non-client button-down and the modal loop, and while
                    // either is active the popup stays open.
                    let interacting = IN_MOVE_OR_RESIZE.load(Ordering::SeqCst)
                        || NC_BUTTON_DOWN.load(Ordering::SeqCst);
                    if !interacting {
                        let _ = hide_popup_dismissed(&app);
                    }
                }
            });
        }
    }
    vibrancy::apply(window)
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}
