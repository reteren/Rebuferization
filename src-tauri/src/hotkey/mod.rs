//! Global hotkey registration.
//!
//! OWNER: worker W3. Two paths: `RegisterHotKey` (default, cheap, reliable)
//! and an opt-in `WH_KEYBOARD_LL` hook for chords Windows already owns, such
//! as `Win+V`.
//!
//! Architecture: a dedicated thread owns a hidden message-only window. The
//! default path registers the chord on that window, so `WM_HOTKEY` arrives on
//! the thread's message loop and fires the trigger callback. The aggressive
//! path instead runs a `WH_KEYBOARD_LL` hook on its own thread whose callback
//! only compares the chord against atomics and posts a `WM_APP` message to the
//! hidden window, which then fires the same callback. `rebind` moves between
//! the two paths at runtime.

pub mod llhook;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, VK_CAPITAL, VK_DELETE, VK_ESCAPE, VK_F4, VK_HOME, VK_END, VK_INSERT,
    VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP, VK_DOWN, VK_BACK, VK_RETURN,
    VK_SNAPSHOT, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7,
    VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_NUMLOCK, VK_ADD, VK_SUBTRACT,
    VK_MULTIPLY, VK_DIVIDE, VK_DECIMAL, VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4,
    VK_NUMPAD5, VK_NUMPAD6, VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, SendMessageW,
    TranslateMessage, WNDCLASSW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY,
};
use windows::core::{w, PCWSTR};

use crate::error::{AppError, AppResult};

/// Message id of the hidden hotkey window; any value within 0x0000..=0xBFFF.
const HOTKEY_ID: i32 = 0x4000;

/// Posted by the low-level hook when it swallows the chord.
const WM_APP_TRIGGER: u32 = WM_APP + 1;

/// Sent (synchronously) by `rebind` so the window thread re-registers the
/// chord; `wparam != 0` selects the aggressive path.
const WM_APP_REBIND: u32 = WM_APP + 2;

const HOTKEY_CLASS: PCWSTR = w!("RebufferHotkeyWnd");

/// The single manager instance. Raw `WNDPROC`/`HOOKPROC` callbacks cannot
/// capture, so they reach the shared state through this process-global.
static MANAGER: OnceLock<Arc<Shared>> = OnceLock::new();

/// Chord state shared between the window thread, the hook thread, and any
/// thread calling `rebind`. Everything the hook callback touches is an atomic
/// so it never blocks on a lock held by another thread.
pub(crate) struct Shared {
    on_trigger: Box<dyn Fn() + Send + Sync + 'static>,
    ctrl: AtomicBool,
    alt: AtomicBool,
    shift: AtomicBool,
    win: AtomicBool,
    vk: AtomicU32,
    aggressive: AtomicBool,
    hwnd: AtomicIsize,
    window_tid: AtomicU32,
    hook: AtomicIsize,
    hook_armed: AtomicBool,
    hook_tid: AtomicU32,
    quit_hook: AtomicBool,
}

impl Shared {
    fn store_chord(&self, chord: &Chord) {
        self.ctrl.store(chord.ctrl, Ordering::SeqCst);
        self.alt.store(chord.alt, Ordering::SeqCst);
        self.shift.store(chord.shift, Ordering::SeqCst);
        self.win.store(chord.win, Ordering::SeqCst);
        self.vk.store(chord.vk, Ordering::SeqCst);
    }
}

/// A parsed chord such as `Alt+V`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Virtual-key code.
    pub vk: u32,
}

impl Chord {
    /// Parses `"Alt+V"`, `"Ctrl+Shift+C"`, `"Win+V"`. Case-insensitive.
    pub fn parse(s: &str) -> AppResult<Chord> {
        let mut chord = Chord { ctrl: false, alt: false, shift: false, win: false, vk: 0 };
        let mut key_seen = false;
        for part in s.split('+') {
            let part = part.trim();
            if part.is_empty() {
                return Err(AppError::Other(format!("invalid hotkey \"{s}\": empty part")));
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "alt" => chord.alt = true,
                "shift" => chord.shift = true,
                "win" | "windows" | "super" | "meta" | "cmd" => chord.win = true,
                _ => {
                    if key_seen {
                        return Err(AppError::Other(format!(
                            "invalid hotkey \"{s}\": more than one key"
                        )));
                    }
                    let vk = parse_key(part).ok_or_else(|| {
                        AppError::Other(format!("invalid hotkey \"{s}\": unknown key \"{part}\""))
                    })?;
                    chord.vk = vk;
                    key_seen = true;
                }
            }
        }
        if !key_seen {
            return Err(AppError::Other(format!("invalid hotkey \"{s}\": no key")));
        }
        Ok(chord)
    }

    /// Canonical display form, always in Ctrl+Alt+Shift+Win+Key order.
    pub fn to_display(&self) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        let key = key_name(self.vk);
        parts.push(&key);
        parts.join("+")
    }

    /// True for combinations Windows itself claims, which `RegisterHotKey`
    /// will refuse. The settings UI shows these as an inline explanation.
    pub fn is_system_reserved(&self) -> bool {
        if !self.win {
            return match (self.ctrl, self.alt, self.shift) {
                (true, true, _) => self.vk == VK_DELETE.0 as u32, // Ctrl+Alt+Del (SAS)
                (true, false, true) => self.vk == VK_ESCAPE.0 as u32, // Ctrl+Shift+Esc
                (true, false, false) => self.vk == VK_ESCAPE.0 as u32, // Ctrl+Esc
                (false, true, false) => {
                    matches!(
                        self.vk,
                        x if x == VK_TAB.0 as u32
                            || x == VK_ESCAPE.0 as u32
                            || x == VK_SPACE.0 as u32
                            || x == VK_F4.0 as u32
                    )
                }
                _ => false,
            };
        }
        // The shell claims every Win+letter (Start menu accelerators), the
        // Win+digit taskbar slots, F1..F12, and a handful of navigation keys.
        let letter = (0x41..=0x5A).contains(&self.vk);
        let digit = (0x30..=0x39).contains(&self.vk);
        let fkey = (0x70..=0x7B).contains(&self.vk);
        let named = matches!(
            self.vk,
            x if x == VK_TAB.0 as u32
                || x == VK_SPACE.0 as u32
                || x == VK_ESCAPE.0 as u32
                || x == VK_SNAPSHOT.0 as u32
                || x == VK_OEM_PERIOD.0 as u32
                || x == VK_OEM_COMMA.0 as u32
                || x == VK_LEFT.0 as u32
                || x == VK_RIGHT.0 as u32
                || x == VK_UP.0 as u32
                || x == VK_DOWN.0 as u32
                || x == VK_HOME.0 as u32
                || x == VK_END.0 as u32
                || x == VK_PRIOR.0 as u32
                || x == VK_NEXT.0 as u32
        );
        letter || digit || fkey || named
    }
}

/// Maps a key token to its virtual-key code: `a`-`z`, `0`-`9`, `F1`-`F24`,
/// and the named keys the settings UI offers.
fn parse_key(token: &str) -> Option<u32> {
    let low = token.to_ascii_lowercase();
    let mut chars = low.chars();
    if let Some(c) = chars.next() {
        if chars.next().is_none() && c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u32);
        }
    }
    if let Some(n) = low.strip_prefix('f') {
        if let Ok(n) = n.parse::<u32>() {
            if (1..=24).contains(&n) {
                return Some(0x70 + n - 1);
            }
        }
        return None;
    }
    let vk = match low.as_str() {
        "space" => VK_SPACE.0 as u32,
        "tab" => VK_TAB.0 as u32,
        "enter" | "return" => VK_RETURN.0 as u32,
        "esc" | "escape" => VK_ESCAPE.0 as u32,
        "backspace" => VK_BACK.0 as u32,
        "delete" | "del" => VK_DELETE.0 as u32,
        "insert" | "ins" => VK_INSERT.0 as u32,
        "home" => VK_HOME.0 as u32,
        "end" => VK_END.0 as u32,
        "pageup" | "pgup" => VK_PRIOR.0 as u32,
        "pagedown" | "pgdn" => VK_NEXT.0 as u32,
        "up" => VK_UP.0 as u32,
        "down" => VK_DOWN.0 as u32,
        "left" => VK_LEFT.0 as u32,
        "right" => VK_RIGHT.0 as u32,
        "printscreen" | "prtscn" | "print" => VK_SNAPSHOT.0 as u32,
        "capslock" => VK_CAPITAL.0 as u32,
        "." => VK_OEM_PERIOD.0 as u32,
        "," => VK_OEM_COMMA.0 as u32,
        ";" => VK_OEM_1.0 as u32,
        "'" => VK_OEM_7.0 as u32,
        "`" => VK_OEM_3.0 as u32,
        "-" => VK_OEM_MINUS.0 as u32,
        "=" => VK_OEM_PLUS.0 as u32,
        "[" => VK_OEM_4.0 as u32,
        "]" => VK_OEM_6.0 as u32,
        "\\" => VK_OEM_5.0 as u32,
        "/" => VK_OEM_2.0 as u32,
        "numlock" => VK_NUMLOCK.0 as u32,
        "num0" => VK_NUMPAD0.0 as u32,
        "num1" => VK_NUMPAD1.0 as u32,
        "num2" => VK_NUMPAD2.0 as u32,
        "num3" => VK_NUMPAD3.0 as u32,
        "num4" => VK_NUMPAD4.0 as u32,
        "num5" => VK_NUMPAD5.0 as u32,
        "num6" => VK_NUMPAD6.0 as u32,
        "num7" => VK_NUMPAD7.0 as u32,
        "num8" => VK_NUMPAD8.0 as u32,
        "num9" => VK_NUMPAD9.0 as u32,
        "numadd" | "numpad+" => VK_ADD.0 as u32,
        "numsub" | "numpad-" => VK_SUBTRACT.0 as u32,
        "nummul" | "numpad*" => VK_MULTIPLY.0 as u32,
        "numdiv" | "numpad/" => VK_DIVIDE.0 as u32,
        "numdec" | "numpad." => VK_DECIMAL.0 as u32,
        _ => return None,
    };
    Some(vk)
}

/// Inverse of `parse_key`, for `to_display`.
fn key_name(vk: u32) -> String {
    if (0x41..=0x5A).contains(&vk) || (0x30..=0x39).contains(&vk) {
        return char::from_u32(vk).unwrap_or('?').to_string();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    let name = match vk {
        x if x == VK_SPACE.0 as u32 => "Space",
        x if x == VK_TAB.0 as u32 => "Tab",
        x if x == VK_RETURN.0 as u32 => "Enter",
        x if x == VK_ESCAPE.0 as u32 => "Esc",
        x if x == VK_BACK.0 as u32 => "Backspace",
        x if x == VK_DELETE.0 as u32 => "Delete",
        x if x == VK_INSERT.0 as u32 => "Insert",
        x if x == VK_HOME.0 as u32 => "Home",
        x if x == VK_END.0 as u32 => "End",
        x if x == VK_PRIOR.0 as u32 => "PageUp",
        x if x == VK_NEXT.0 as u32 => "PageDown",
        x if x == VK_UP.0 as u32 => "Up",
        x if x == VK_DOWN.0 as u32 => "Down",
        x if x == VK_LEFT.0 as u32 => "Left",
        x if x == VK_RIGHT.0 as u32 => "Right",
        x if x == VK_SNAPSHOT.0 as u32 => "PrtScn",
        x if x == VK_CAPITAL.0 as u32 => "CapsLock",
        x if x == VK_OEM_PERIOD.0 as u32 => ".",
        x if x == VK_OEM_COMMA.0 as u32 => ",",
        x if x == VK_OEM_1.0 as u32 => ";",
        x if x == VK_OEM_7.0 as u32 => "'",
        x if x == VK_OEM_3.0 as u32 => "`",
        x if x == VK_OEM_MINUS.0 as u32 => "-",
        x if x == VK_OEM_PLUS.0 as u32 => "=",
        x if x == VK_OEM_4.0 as u32 => "[",
        x if x == VK_OEM_6.0 as u32 => "]",
        x if x == VK_OEM_5.0 as u32 => "\\",
        x if x == VK_OEM_2.0 as u32 => "/",
        x if x == VK_NUMLOCK.0 as u32 => "NumLock",
        x if x == VK_ADD.0 as u32 => "Num+",
        x if x == VK_SUBTRACT.0 as u32 => "Num-",
        x if x == VK_MULTIPLY.0 as u32 => "Num*",
        x if x == VK_DIVIDE.0 as u32 => "Num/",
        x if x == VK_DECIMAL.0 as u32 => "Num.",
        x if (VK_NUMPAD0.0 as u32..=VK_NUMPAD9.0 as u32).contains(&x) => {
            return format!("Num{}", x - VK_NUMPAD0.0 as u32);
        }
        _ => return format!("Key({vk})"),
    };
    name.to_string()
}

/// Holds whichever registration path is active. Dropping it unregisters.
pub struct HotkeyManager {
    shared: Arc<Shared>,
    window_thread: Option<JoinHandle<()>>,
    hook_thread: Mutex<Option<JoinHandle<()>>>,
}

impl HotkeyManager {
    /// Starts the hidden-window message loop. No chord is bound until
    /// [`Self::rebind`] is called with the settings chord.
    pub fn new(on_trigger: Box<dyn Fn() + Send + Sync + 'static>) -> AppResult<HotkeyManager> {
        let shared = Arc::new(Shared {
            on_trigger,
            ctrl: AtomicBool::new(false),
            alt: AtomicBool::new(false),
            shift: AtomicBool::new(false),
            win: AtomicBool::new(false),
            vk: AtomicU32::new(0),
            aggressive: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
            window_tid: AtomicU32::new(0),
            hook: AtomicIsize::new(0),
            hook_armed: AtomicBool::new(true),
            hook_tid: AtomicU32::new(0),
            quit_hook: AtomicBool::new(false),
        });
        MANAGER
            .set(shared.clone())
            .map_err(|_| AppError::Other("hotkey manager already created".into()))?;

        let (tx, rx) = mpsc::channel();
        let window_thread = spawn_window_thread(shared.clone(), tx)
            .map_err(|e| AppError::Other(format!("cannot spawn hotkey thread: {e}")))?;
        let status = rx
            .recv_timeout(Duration::from_millis(2000))
            .map_err(|_| AppError::Other("hotkey window thread did not start".into()))?;
        status.map_err(AppError::Other)?;

        Ok(HotkeyManager { shared, window_thread: Some(window_thread), hook_thread: Mutex::new(None) })
    }

    /// Swaps the binding at runtime, as the settings window does. `aggressive`
    /// selects the low-level hook path.
    pub fn rebind(&self, chord: &Chord, aggressive: bool) -> AppResult<()> {
        let shared = &self.shared;
        // Serialize concurrent rebinds; also guards the hook lifecycle (spawn
        // and join) against double-install races.
        let mut hook_guard = self.hook_thread.lock();

        // Leaving aggressive mode: stop the hook first, so the chord is never
        // swallowed by the hook while RegisterHotKey is also active (which
        // would fire the trigger twice).
        if !aggressive {
            if let Some(thread) = hook_guard.take() {
                llhook::request_stop(shared);
                let _ = thread.join();
            }
        }

        shared.store_chord(chord);
        shared.hook_armed.store(true, Ordering::SeqCst);
        shared.aggressive.store(aggressive, Ordering::SeqCst);

        let hwnd = HWND(shared.hwnd.load(Ordering::SeqCst) as *mut c_void);
        if hwnd.is_invalid() {
            return Err(AppError::Other("hotkey window missing".into()));
        }
        // Synchronous: the window thread re-registers before we return.
        // FFI: hwnd is the live hidden window; wparam/lparam are plain words.
        unsafe {
            SendMessageW(hwnd, WM_APP_REBIND, Some(WPARAM(usize::from(aggressive))), None);
        }

        if aggressive && shared.hook.load(Ordering::SeqCst) == 0 {
            let (tx, rx) = mpsc::channel();
            let handle = llhook::install(shared.clone(), tx)
                .map_err(|e| AppError::Other(format!("cannot spawn hook thread: {e}")))?;
            *hook_guard = Some(handle);
            match rx.recv_timeout(Duration::from_millis(2000)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    // Degrade to the standard path and report; better a
                    // working non-reserved chord than a dead hotkey.
                    tracing::warn!("low-level keyboard hook failed, falling back: {e}");
                    shared.aggressive.store(false, Ordering::SeqCst);
                    // FFI: same valid hwnd as above; rolls registration back.
                    unsafe {
                        SendMessageW(hwnd, WM_APP_REBIND, Some(WPARAM(0)), None);
                    }
                    return Err(AppError::Other(format!(
                        "low-level keyboard hook could not be installed: {e}"
                    )));
                }
                Err(_) => {
                    return Err(AppError::Other("low-level keyboard hook did not start".into()));
                }
            }
        }
        Ok(())
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let shared = &self.shared;
        if let Some(thread) = self.hook_thread.lock().take() {
            llhook::request_stop(shared);
            let _ = thread.join();
        }
        let hwnd = HWND(shared.hwnd.load(Ordering::SeqCst) as *mut c_void);
        if !hwnd.is_invalid() {
            // FFI: hwnd is the live hidden window; unregister is idempotent.
            unsafe {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
            }
        }
        let tid = shared.window_tid.load(Ordering::SeqCst);
        if tid != 0 {
            // FFI: posting WM_QUIT to our own thread's id is always valid.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                    tid,
                    windows::Win32::UI::WindowsAndMessaging::WM_QUIT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        if let Some(thread) = self.window_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Creates the hidden window and pumps messages for it. The thread keeps
/// running until `WM_QUIT` is posted to it.
fn spawn_window_thread(
    shared: Arc<Shared>,
    ready: mpsc::Sender<Result<(), String>>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("rebuffer-hotkey".into())
        .spawn(move || {
            // RegisterClassW is process-global; the `Once` keeps this cheap.
            static CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();
            // FFI: NULL module name yields the process's own module handle.
            let hinstance = unsafe { GetModuleHandleW(PCWSTR::default()) }.unwrap_or_default();
            CLASS_REGISTERED.call_once(|| {
                let class = WNDCLASSW {
                    style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
                    lpfnWndProc: Some(hotkey_wnd_proc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance.into(),
                    hIcon: Default::default(),
                    hCursor: Default::default(),
                    hbrBackground: Default::default(),
                    lpszMenuName: Default::default(),
                    lpszClassName: HOTKEY_CLASS,
                };
                unsafe {
                    // FFI: the class struct is a stack value valid for the call.
                    // RegisterClassW failure is fine: the class may already
                    // exist from a previous run of this function.
                    let _ = RegisterClassW(&class);
                }
            });
            // FFI: the class was registered above; all params are plain values.
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    HOTKEY_CLASS,
                    w!("RebufferHotkey"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(hinstance.into()),
                    None,
                )
            };
            match hwnd {
                Ok(hwnd) => {
                    shared.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
                    // FFI: takes no arguments, always succeeds.
                    shared
                        .window_tid
                        .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
                    let _ = ready.send(Ok(()));
                    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                    loop {
                        // FFI: msg is a valid stack buffer; returns are checked.
                        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                        if ret.0 == 0 || ret.0 == -1 {
                            break; // WM_QUIT or a fatal error
                        }
                        unsafe {
                            // FFI: msg is filled by GetMessageW above and valid here.
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
                Err(e) => {
                    let _ = ready.send(Err(e.message().to_string()));
                }
            }
        })
}

/// Handles `WM_HOTKEY` from the standard path and `WM_APP_TRIGGER` posted by
/// the low-level hook; both fire the manager's callback.
unsafe extern "system" fn hotkey_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_HOTKEY | WM_APP_TRIGGER => {
            if let Some(shared) = MANAGER.get() {
                (shared.on_trigger)();
            }
            LRESULT(0)
        }
        WM_APP_REBIND => {
            if let Some(shared) = MANAGER.get() {
                // wparam != 0 selects the aggressive (hook) path.
                apply_registration(shared, wparam.0 != 0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }, // FFI: standard default handling
    }
}

/// Runs on the window thread: swaps `RegisterHotKey` on or off to match the
/// currently selected path. A no-op chord (vk 0, before the first `rebind`)
/// registers nothing.
fn apply_registration(shared: &Shared, aggressive: bool) {
    let hwnd = HWND(shared.hwnd.load(Ordering::SeqCst) as *mut c_void);
    if hwnd.is_invalid() {
        return;
    }
    // FFI: hwnd is the live hidden window; mods/vk come from our own atomics.
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
        if aggressive {
            return;
        }
        let vk = shared.vk.load(Ordering::SeqCst);
        if vk == 0 {
            return;
        }
        let mut mods = HOT_KEY_MODIFIERS(0);
        if shared.ctrl.load(Ordering::SeqCst) {
            mods |= MOD_CONTROL;
        }
        if shared.alt.load(Ordering::SeqCst) {
            mods |= MOD_ALT;
        }
        if shared.shift.load(Ordering::SeqCst) {
            mods |= MOD_SHIFT;
        }
        if shared.win.load(Ordering::SeqCst) {
            mods |= MOD_WIN;
        }
        // MOD_NOREPEAT so a held key never re-fires the popup.
        match RegisterHotKey(Some(hwnd), HOTKEY_ID, mods | MOD_NOREPEAT, vk) {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(
                    "RegisterHotKey refused ({}); the settings UI explains reserved chords",
                    e
                );
            }
        }
    }
}