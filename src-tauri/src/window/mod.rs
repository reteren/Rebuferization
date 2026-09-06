//! Popup placement, backdrop, focus handling, and paste injection.
//!
//! OWNER: worker W3.

pub mod memory;
pub mod paste;
pub mod position;
pub mod vibrancy;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId,
    SetForegroundWindow, SetWindowLongPtrW, GWL_EXSTYLE, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE,
    WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use crate::error::{AppError, AppResult};
use crate::AppState;

pub const POPUP_LABEL: &str = "popup";
pub const SETTINGS_LABEL: &str = "settings";
pub const TRAYMENU_LABEL: &str = "traymenu";

/// The WebView2 command line every one of our windows is created with.
///
/// `--renderer-process-limit=1` asks Chromium to keep one renderer instead of
/// one per window. It does not fully deliver that, and it is worth being
/// precise about why: it is an internal Chromium testing switch, not a
/// supported WebView2 API, and two things keep extra renderers alive anyway —
/// Chromium holds a warm spare renderer ready for the next navigation, and a
/// window created while an earlier renderer is still initialising cannot bind
/// to it and gets its own. Measured here: 10 processes and 192.4 MB became 9
/// and 181.3 MB. One process, not two, and the exact number is timing
/// dependent. Being unsupported, a future WebView2 may ignore it; that is a
/// benign failure, we simply get the extra renderer back.
///
/// The rest is not optional decoration. Setting additional browser arguments
/// *replaces* the string wry passes by default (wry 0.55.1 only builds its
/// default when none was supplied), so wry's own
/// `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` (the
/// WebView2 "mini menu" and SmartScreen) has to be repeated here or it would
/// silently come back.
///
/// Every window must pass this same string, and that is a hard requirement
/// rather than tidiness. WebView2 runs one browser process per user-data
/// folder and fixes its arguments when that process starts; a second
/// environment on the same folder asking for different arguments fails with
/// `ERROR_INVALID_STATE` (0x8007139F). Since the popup is declared in
/// tauri.conf.json and the other two are built here, a divergence between the
/// two spellings would not be a cosmetic bug — settings and the tray menu
/// would stop opening entirely. `browser_args_match_the_manifest` guards it.
pub const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --renderer-process-limit=1";

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

/// The popup's client size when a move/size drag began, so the end of the drag
/// can tell a resize from a plain move. With the drag bar on, moving the window
/// is an everyday action, and a move must not be mistaken for a resize — that
/// would rewrite `sizeMode` to `"fixed"` and silently drop a percent-of-monitor
/// setting the user had chosen.
static SIZE_AT_MOVE_START: Mutex<Option<(i32, i32)>> = Mutex::new(None);

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
            *SIZE_AT_MOVE_START.lock().unwrap_or_else(|e| e.into_inner()) = client_size(hwnd);
        }
        WM_EXITSIZEMOVE => {
            IN_MOVE_OR_RESIZE.store(false, Ordering::SeqCst);
            NC_BUTTON_DOWN.store(false, Ordering::SeqCst);
            let before = SIZE_AT_MOVE_START
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            let resized = before.is_none() || before != client_size(hwnd);
            if resized {
                if let Some(app) = POPUP_APP.get() {
                    persist_resized_size(app.clone());
                }
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

/// The popup's client size in physical pixels, or `None` if Windows would not
/// say.
fn client_size(hwnd: HWND) -> Option<(i32, i32)> {
    let mut r = RECT::default();
    // FFI: hwnd is the live popup window and `r` is a plain out-param.
    if unsafe { GetClientRect(hwnd, &mut r) }.is_ok() {
        Some((r.right - r.left, r.bottom - r.top))
    } else {
        None
    }
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
        // The configured height excludes the drag bar — `window_size` adds it
        // back on every show. Storing the measured height as-is would add the
        // bar a second time, and the popup would grow by its height on every
        // resize.
        let height = if state.settings.get().window.drag_bar {
            height.saturating_sub(position::DRAG_BAR_HEIGHT).max(1)
        } else {
            height
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

/// Claimed for the length of a show, so that two concurrent ones cannot both
/// read `is_visible` before either acts and have the second hide what the
/// first just put on screen.
///
/// Deliberately an atomic claim and not a mutex, for the same reason
/// `get_or_build` is not one. `show_popup` runs on the hotkey thread and on
/// the main thread alike: the tray click handler and the single-instance
/// callback both land on the event loop. It holds the claim across
/// `place_popup`, `show` and `set_focus`, and every one of those has to be
/// carried out by the event loop, blocking the caller until the main thread
/// gets to it. With a mutex, a main thread that blocked on a lock held by the
/// hotkey thread would never service those calls, and neither side could ever
/// finish: the tray menu stops opening, settings stop opening, and the popup
/// can no longer be dismissed, with the process sitting at zero CPU. That was
/// not hypothetical — it reproduced in a soak test of Alt+V against repeated
/// second launches, and it stayed hung permanently.
///
/// An atomic claim never makes one thread wait for another. The loser gives up
/// instead, which loses nothing: the show already in flight ends in
/// `set_focus`, which is exactly what the dropped call was asking for.
static SHOWING: AtomicBool = AtomicBool::new(false);

/// Holds the show claim and releases it on every path out of `show_popup`,
/// including the `?` returns.
struct ShowClaim;

impl ShowClaim {
    fn try_claim() -> Option<Self> {
        (!SHOWING.swap(true, Ordering::SeqCst)).then_some(ShowClaim)
    }
}

impl Drop for ShowClaim {
    fn drop(&mut self) {
        SHOWING.store(false, Ordering::SeqCst);
    }
}

/// Caches the foreground `HWND`, positions the popup at the cursor clamped to
/// the work area, applies the backdrop, and shows it.
pub fn show_popup(app: &AppHandle) -> AppResult<()> {
    let win = app
        .get_webview_window(POPUP_LABEL)
        .ok_or_else(|| AppError::Other("popup window not found".into()))?;

    let Some(_claim) = ShowClaim::try_claim() else {
        // A show is already in flight. It finishes by focusing the popup,
        // which is what this call wanted, so there is nothing left to do.
        return Ok(());
    };
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
    // A show in flight is about to assert focus itself, and the deactivation
    // that brought us here is part of that handover rather than the user
    // clicking away.
    if SHOWING.load(Ordering::SeqCst) {
        return Ok(());
    }
    hide_popup_impl(app, false)
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
    if was_visible {
        // Not immediately: a hide is very often followed by another show
        // (Win+V toggling, a paste that reopens), and trimming pages we are
        // about to fault straight back in would be pure cost.
        memory::schedule_trim(app, Duration::from_secs(5));
    }
    Ok(())
}

/// True when `hwnd` is one of our own windows, which must never be treated as
/// a paste target — SPEC 5.4 calls pasting into ourselves the destructive
/// case. All three labels belong here, the tray menu included: it takes focus
/// when it opens, so a show that happened while it was frontmost would
/// otherwise cache it as the window to paste into. A lazily built window that
/// does not currently exist simply matches nothing.
fn is_own_window(app: &AppHandle, hwnd: HWND) -> bool {
    [POPUP_LABEL, SETTINGS_LABEL, TRAYMENU_LABEL]
        .iter()
        .any(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .is_some_and(|own| own == hwnd)
        })
}

/// A window that is built the first time it is needed and destroyed again
/// once it has been idle long enough.
///
/// The settings and tray-menu webviews used to be declared in tauri.conf.json
/// and created hidden at startup. That made them instant to open and cost
/// ~28 MB of resident memory for the whole life of a process that sits idle in
/// the tray almost all of the time. They are built on demand now, and torn
/// down again afterwards so the saving does not evaporate the first time the
/// user opens settings.
struct Lazy {
    label: &'static str,
    /// Bumped on every show. A pending destroy carries the generation it was
    /// scheduled under and gives up when it no longer matches — that is how a
    /// show cancels a teardown that was already in flight.
    generation: AtomicU64,
    /// True between "the window was built" and "its page has painted". The
    /// reveal is deferred to the page-load hook, and this says one is owed.
    pending_show: AtomicBool,
    /// At most one idle timer per window, however often it is hidden.
    timer: AtomicBool,
    /// True while a build of this window is in flight. Claiming it is how two
    /// concurrent shows agree on which one builds; see `get_or_build`.
    building: AtomicBool,
    /// How long the window may sit hidden before it is destroyed, or `None`
    /// for a window that is built once and then kept for the life of the
    /// process. Keeping one is not free, but after a working-set trim a hidden
    /// renderer costs about a megabyte, and rebuilding costs 300-600 ms the
    /// next time the user asks for it — which is the wrong trade for anything
    /// that has to feel instant.
    idle: Option<Duration>,
    /// Positions, shows and focuses the window. Runs once the page is ready.
    reveal: fn(&WebviewWindow) -> AppResult<()>,
}

/// Settings is a whole window the user works in — the largest of the three
/// renderers — and it is opened rarely. Five minutes is long enough that
/// coming back to it during one sitting is still instant, and taking 400 ms to
/// open a settings window is unremarkable.
static SETTINGS: Lazy = Lazy {
    label: SETTINGS_LABEL,
    generation: AtomicU64::new(0),
    pending_show: AtomicBool::new(false),
    timer: AtomicBool::new(false),
    building: AtomicBool::new(false),
    idle: Some(Duration::from_secs(300)),
    reveal: reveal_settings,
};

/// The tray menu is built on demand but never torn down again.
///
/// Tearing it down was tried and reverted. Measured: once the working set has
/// been trimmed the hidden tray-menu renderer holds about 1.3 MB, while
/// rebuilding it costs 300-600 ms — paid on a right-click, on a menu whose
/// entire job is to appear immediately. A megabyte does not buy half a second
/// of lag on the one window that must feel instant. Lazy creation is still
/// worth keeping: a user who never opens the menu never pays for it at all.
static TRAY_MENU: Lazy = Lazy {
    label: TRAYMENU_LABEL,
    generation: AtomicU64::new(0),
    pending_show: AtomicBool::new(false),
    timer: AtomicBool::new(false),
    building: AtomicBool::new(false),
    idle: None,
    reveal: reveal_tray_menu,
};

/// Looks the window up and builds it if it is not there.
///
/// Returns the window when it already existed, so the caller reveals it
/// immediately; returns `None` when a build was started or is already running,
/// in which case the builder's page-load hook (or `arm_reveal_fallback`) does
/// the revealing once there is something painted to show.
///
/// Two concurrent triggers — a double right-click on the tray icon, or the
/// tray menu's Settings entry while the tray icon is clicked again — would
/// otherwise both see `get_webview_window` return `None`, both build, and the
/// second `WebviewWindowBuilder::build` would fail with
/// `WindowLabelAlreadyExists`. `building` is claimed with a single swap so
/// exactly one of them proceeds.
///
/// Deliberately NOT a mutex. Creating a window has to happen on the event loop
/// thread, so `build` called from a command thread blocks until the main
/// thread services it. A main thread that meanwhile blocked on a mutex held by
/// that command thread would never service anything, and the app would hang —
/// the same trap the `SHOWING` comment describes. An atomic claim never
/// makes a thread wait for another, so there is nothing to deadlock on: the
/// loser simply returns and lets the winner finish.
fn get_or_build(
    lazy: &'static Lazy,
    app: &AppHandle,
    build: fn(&AppHandle) -> AppResult<WebviewWindow>,
) -> AppResult<Option<WebviewWindow>> {
    if let Some(win) = app.get_webview_window(lazy.label) {
        return Ok(Some(win));
    }
    if lazy.building.swap(true, Ordering::SeqCst) {
        // Someone else is building it right now and owes the reveal.
        return Ok(None);
    }
    // Re-check now that the build slot is ours: the window may have been
    // finished between our look-up above and this claim.
    if let Some(win) = app.get_webview_window(lazy.label) {
        lazy.building.store(false, Ordering::SeqCst);
        return Ok(Some(win));
    }
    lazy.pending_show.store(true, Ordering::SeqCst);
    let built = build(app);
    lazy.building.store(false, Ordering::SeqCst);
    match built {
        Ok(win) => {
            arm_reveal_fallback(lazy, &win);
            Ok(None)
        }
        Err(e) => {
            // Nothing will ever reveal it now; drop the debt so a later show
            // is not mistaken for one already owed.
            lazy.pending_show.store(false, Ordering::SeqCst);
            Err(e)
        }
    }
}

impl Lazy {
    /// Records that the window is on screen, cancelling any pending destroy.
    fn shown(&'static self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Starts the idle countdown. A show during the wait restarts it. A window
    /// with no idle period is kept for good, so there is nothing to count.
    fn hidden(&'static self, app: &AppHandle) {
        let Some(idle) = self.idle else {
            return;
        };
        if self.timer.swap(true, Ordering::SeqCst) {
            // A countdown is already running; it re-reads the generation on
            // every lap, so it notices this hide by itself.
            return;
        }
        let app = app.clone();
        std::thread::spawn(move || {
            let generation = loop {
                let generation = self.generation.load(Ordering::SeqCst);
                std::thread::sleep(idle);
                if self.generation.load(Ordering::SeqCst) == generation {
                    break generation;
                }
                // Shown again while we waited: start the idle stretch over.
            };
            self.timer.store(false, Ordering::SeqCst);
            let handle = app.clone();
            // Windows destroys a window on the thread that owns it, which is
            // the main thread; doing it from here would be undefined.
            if let Err(e) = app.run_on_main_thread(move || {
                destroy_if_idle(self, &handle, generation);
            }) {
                tracing::debug!("{}: could not reach the main thread: {e}", self.label);
            }
        });
    }
}

/// Destroys the window if nothing has happened to it since the countdown
/// started. Main thread only.
fn destroy_if_idle(lazy: &'static Lazy, app: &AppHandle, generation: u64) {
    if lazy.generation.load(Ordering::SeqCst) != generation {
        return;
    }
    let Some(win) = app.get_webview_window(lazy.label) else {
        return;
    };
    // A show is owed: the window was built moments ago and is waiting for its
    // page-load hook, with `arm_reveal_fallback` asleep holding a clone of it.
    // Destroying now would leave that fallback to reveal a dead window.
    if lazy.pending_show.load(Ordering::SeqCst) {
        return;
    }
    // Never destroy a window the user is looking at. A visibility read that
    // fails is treated as "visible": leaving the memory in place is always the
    // safe answer.
    if win.is_visible().unwrap_or(true) {
        return;
    }
    // `close()` would go through the CloseRequested handler, which exists
    // precisely to refuse it; `destroy()` is the one that actually takes the
    // renderer process down with it.
    match win.destroy() {
        Ok(()) => {
            tracing::debug!("{} destroyed after {:?} idle", lazy.label, lazy.idle);
            // The renderer needs a moment to exit before there is anything to
            // reclaim.
            memory::schedule_trim(app, Duration::from_secs(2));
        }
        Err(e) => tracing::debug!("{} could not be destroyed: {e}", lazy.label),
    }
}

/// How long to wait for a freshly built window to report a finished page load
/// before showing it regardless.
const REVEAL_TIMEOUT: Duration = Duration::from_millis(2000);

/// A lazily built window is revealed from its page-load hook. If that never
/// arrives the window would sit there built and hidden, and the click that
/// asked for it would look ignored — so show it anyway after a moment.
fn arm_reveal_fallback(lazy: &'static Lazy, win: &WebviewWindow) {
    let win = win.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REVEAL_TIMEOUT);
        if !lazy.pending_show.swap(false, Ordering::SeqCst) {
            return;
        }
        tracing::warn!(
            "{} never reported a finished page load; showing it anyway",
            lazy.label
        );
        if let Err(e) = (lazy.reveal)(&win) {
            tracing::error!("{} could not be shown: {e}", lazy.label);
        }
    });
}

/// Runs from the page-load hook of a lazily built window: shows it if a show
/// is owed, and does nothing on any later navigation.
fn reveal_when_loaded(lazy: &'static Lazy, win: &WebviewWindow, event: PageLoadEvent) {
    if event != PageLoadEvent::Finished || !lazy.pending_show.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = (lazy.reveal)(win) {
        tracing::error!("{} could not be shown: {e}", lazy.label);
    }
}

/// The settings window, built if it is not there.
///
/// The properties are the ones tauri.conf.json declared for it before it
/// became lazy — the same title, size and minimum size, and the same
/// undecorated, transparent, shadowed, taskbar-less window.
fn ensure_settings(app: &AppHandle) -> AppResult<WebviewWindow> {
    if let Some(win) = app.get_webview_window(SETTINGS_LABEL) {
        return Ok(win);
    }
    let win =
        WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
            .title("Rebuffer — Settings")
            .inner_size(960.0, 660.0)
            .min_inner_size(720.0, 520.0)
            .visible(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(true)
            .center()
            .skip_taskbar(true)
            .additional_browser_args(BROWSER_ARGS)
            .on_page_load(|win, payload| reveal_when_loaded(&SETTINGS, &win, payload.event()))
            .build()
            .map_err(tauri_err)?;
    apply_backdrop(&win)?;
    Ok(win)
}

/// The tray-menu window, built if it is not there. Again the properties the
/// JSON declared: a fixed 210x132, always on top, focused, no taskbar button.
fn ensure_tray_menu(app: &AppHandle) -> AppResult<WebviewWindow> {
    if let Some(win) = app.get_webview_window(TRAYMENU_LABEL) {
        return Ok(win);
    }
    let win =
        WebviewWindowBuilder::new(app, TRAYMENU_LABEL, WebviewUrl::App("traymenu.html".into()))
            .title("Rebuffer menu")
            .inner_size(210.0, 132.0)
            .visible(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .focused(true)
            .additional_browser_args(BROWSER_ARGS)
            .on_page_load(|win, payload| reveal_when_loaded(&TRAY_MENU, &win, payload.event()))
            .build()
            .map_err(tauri_err)?;
    apply_backdrop(&win)?;
    Ok(win)
}

/// Where the tray menu should appear, captured when the right-click happened.
/// A window that has to be built first is revealed a few hundred milliseconds
/// later, and by then the pointer is often already moving toward where the
/// menu is about to be; placing it at the cursor *then* would make the menu
/// jump away from the click that asked for it.
static TRAYMENU_ORIGIN: Mutex<Option<(i32, i32)>> = Mutex::new(None);

/// Shows the tray menu at the cursor. It is our own window rather than a
/// native one because a native HMENU cannot be themed: Windows paints it, and
/// no amount of CSS reaches it. The cost is that dismissal, sizing and
/// placement are ours to handle.
pub fn show_tray_menu(app: &AppHandle) -> AppResult<()> {
    let at = position::cursor_pos()?;
    *TRAYMENU_ORIGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some((at.x, at.y));

    // A window that has to be built is left hidden and shown from its
    // page-load hook: an empty transparent window at the cursor would be a
    // hole in the screen until the menu painted itself.
    match get_or_build(&TRAY_MENU, app, ensure_tray_menu)? {
        Some(win) => reveal_tray_menu(&win),
        None => Ok(()),
    }
}

fn reveal_tray_menu(win: &WebviewWindow) -> AppResult<()> {
    TRAY_MENU.shown();
    // Read the origin out and drop the guard before placing the window. Held
    // across the `match`, as a lock in the scrutinee position is, it would
    // still be held during `place_at` — which moves a window and therefore
    // blocks until the event loop runs it. This function also runs on the
    // reveal-fallback thread, so that would let the main thread deadlock on
    // this mutex inside `show_tray_menu`, and the menu would stop opening for
    // the rest of the session.
    let origin = *TRAYMENU_ORIGIN.lock().unwrap_or_else(|e| e.into_inner());
    match origin {
        Some((x, y)) => position::place_at(win, POINT { x, y })?,
        None => position::place_at_cursor(win)?,
    }
    win.show().map_err(tauri_err)?;
    win.set_focus().map_err(tauri_err)?;
    Ok(())
}

pub fn hide_tray_menu(app: &AppHandle) -> AppResult<()> {
    if let Some(win) = app.get_webview_window(TRAYMENU_LABEL) {
        // Only a window that was actually on screen has been hidden by this
        // call. Arming the idle countdown from an already-hidden window — a
        // second dismissal, or an event that `destroy()` itself emits — would
        // keep restarting a timer against a window nobody touched.
        if win.is_visible().unwrap_or(false) {
            win.hide().map_err(tauri_err)?;
            TRAY_MENU.hidden(app);
        }
    }
    Ok(())
}

/// Centers the settings window on the cursor's monitor and shows it, building
/// it first if it is not currently around.
pub fn show_settings(app: &AppHandle) -> AppResult<()> {
    match get_or_build(&SETTINGS, app, ensure_settings)? {
        Some(win) => reveal_settings(&win),
        None => Ok(()),
    }
}

fn reveal_settings(win: &WebviewWindow) -> AppResult<()> {
    SETTINGS.shown();
    position::center_on_cursor_monitor(win)?;
    win.show().map_err(tauri_err)?;
    // After the show, not before: showing the window puts WS_EX_APPWINDOW back,
    // so setting the style once at startup was undone every time.
    //
    // Tauri's own set_skip_taskbar is deliberately not used. It re-shows the
    // window to apply the change, and the deactivation that causes was read by
    // the dismissal below as the user clicking away — the settings window shut
    // itself the moment it opened.
    keep_out_of_taskbar(win);
    *SETTINGS_SHOWN_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
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

/// When the settings window was last shown. Focus settles over a few frames —
/// the popup is still hiding, the shell is still handing activation over — and
/// a deactivation seen in that gap is not the user clicking away.
static SETTINGS_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

const SETTINGS_FOCUS_GRACE: Duration = Duration::from_millis(600);

fn settling_after_show() -> bool {
    matches!(
        *SETTINGS_SHOWN_AT.lock().unwrap_or_else(|e| e.into_inner()),
        Some(t) if t.elapsed() < SETTINGS_FOCUS_GRACE
    )
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

/// Wired once per settings window, from `apply_backdrop` — which is to say
/// once per `ensure_settings`, on the instance it just built. This is
/// deliberately not a process-wide once-guard: the window is destroyed after
/// it has been idle, and a rebuilt one that inherited nothing would be
/// undismissable and unclosable.
///
/// Two things the settings window got wrong before this existed:
///
/// Closing it destroyed the webview, and every later "open settings" then
/// looked up a window that no longer existed and failed — settings could not
/// be reopened at all until the app was restarted. Hiding instead keeps the
/// window alive for as long as it is worth keeping (`Lazy`), and rebuilding is
/// what happens after that.
///
/// And it stayed open behind whatever the user switched to. Losing focus is
/// the dismissal signal, exactly as it is for the popup, except that our own
/// file dialogs must not count as losing it.
fn wire_settings_dismissal(window: &WebviewWindow) {
    let win = window.clone();
    let app = window.app_handle().clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_settings(&win, &app);
        }
        WindowEvent::Focused(false) if !foreground_is_ours() && !settling_after_show() => {
            hide_settings(&win, &app);
        }
        _ => {}
    });
}

/// Hides the settings window and starts its idle countdown. A window that is
/// already hidden is left alone: destroying one emits its own events, and
/// re-arming the countdown from those would keep a dead label alive forever.
fn hide_settings(win: &WebviewWindow, app: &AppHandle) {
    if !win.is_visible().unwrap_or(false) {
        return;
    }
    let _ = win.hide();
    SETTINGS.hidden(app);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn browser_args_in_tauri_conf_must_match_window_mod_constant_to_prevent_webview2_failure() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf_path = manifest_dir.join("tauri.conf.json");
        let raw = std::fs::read_to_string(&conf_path)
            .or_else(|_| std::fs::read_to_string("tauri.conf.json"))
            .or_else(|_| std::fs::read_to_string("../tauri.conf.json"))
            .expect("tauri.conf.json must exist and be readable at test time");

        let val: serde_json::Value =
            serde_json::from_str(&raw).expect("tauri.conf.json must be valid JSON");
        let from_conf = val["app"]["windows"][0]["additionalBrowserArgs"]
            .as_str()
            .expect(
            "app.windows[0].additionalBrowserArgs must be present as a string in tauri.conf.json",
        );

        assert_eq!(
            from_conf, BROWSER_ARGS,
            "browser arguments in tauri.conf.json (app.windows[0].additionalBrowserArgs) and window::BROWSER_ARGS must match exactly. A mismatch causes WebView2 to fail to create secondary windows in the same user data folder."
        );
    }
}
