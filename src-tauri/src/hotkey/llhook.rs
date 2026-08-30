//! The opt-in aggressive hotkey path: a `WH_KEYBOARD_LL` hook.
//!
//! OWNER: worker W3.
//!
//! Windows silently unregisters a low-level hook whose callback exceeds the
//! system timeout, so the callback here does nothing but compare the chord
//! against atomics and post a message: no allocation, no logging, no lock that
//! another thread could hold, no user32 calls that can block. The chord state
//! lives in `super::Shared`, reached through the process-global `MANAGER`.
//!
//! The hook thread runs its own message loop, which is what lets the system
//! invoke the callback in the first place.

use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::sync::{mpsc::Sender, Arc};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_UP,
    MSG, WH_KEYBOARD_LL, WM_QUIT,
};

use crate::error::{AppError, AppResult};

use super::{Shared, MANAGER, WM_APP_TRIGGER};

/// Spawns the hook thread. It installs the hook, reports readiness on `ready`,
/// and pumps messages until told to stop.
pub(crate) fn install(
    shared: Arc<Shared>,
    ready: Sender<Result<(), String>>,
) -> AppResult<JoinHandle> {
    shared.quit_hook.store(false, Ordering::SeqCst);
    shared.hook_armed.store(true, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("rebuffer-llhook".into())
        .spawn(move || run_hook(shared, ready))
        .map_err(|e| AppError::Other(format!("cannot spawn low-level hook thread: {e}")))
}

pub type JoinHandle = std::thread::JoinHandle<()>;

fn run_hook(shared: Arc<Shared>, ready: Sender<Result<(), String>>) {
    // FFI: for WH_KEYBOARD_LL the hmod/dwthreadid are NULL/0, meaning "this
    // process, install from this thread"; the proc is a static fn pointer.
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_hook_proc), None, 0) };
    match hook {
        Ok(hook) => {
            shared.hook.store(hook.0 as isize, Ordering::SeqCst);
            // FFI: takes no arguments, always succeeds.
            shared
                .hook_tid
                .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
            let _ = ready.send(Ok(()));
            pump(shared.clone(), hook);
            // FFI: same-thread uninstall of the hook installed above.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
            shared.hook.store(0, Ordering::SeqCst);
        }
        Err(e) => {
            let _ = ready.send(Err(e.message().to_string()));
        }
    }
}

fn pump(shared: Arc<Shared>, _hook: HHOOK) {
    let mut msg = MSG::default();
    loop {
        if shared.quit_hook.load(Ordering::SeqCst) {
            break; // checked before GetMessageW so a stop raced with thread start still exits
        }
        // FFI: msg is a valid stack buffer; returns are checked.
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 == 0 || ret.0 == -1 {
            break; // WM_QUIT (posted by request_stop) or a fatal error
        }
        unsafe {
            // FFI: msg is filled by GetMessageW above and valid here.
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Asks the hook thread to uninstall and exit.
pub(crate) fn request_stop(shared: &Shared) {
    shared.quit_hook.store(true, Ordering::SeqCst);
    let tid = shared.hook_tid.load(Ordering::SeqCst);
    if tid != 0 {
        // FFI: posting WM_QUIT to our own thread's id is always valid.
        unsafe {
            // WM_QUIT wakes the blocked GetMessageW loop.
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Compares the pressed chord against the registered one and, on a match,
/// swallows the keystroke and posts `WM_APP_TRIGGER` to the hidden window.
/// Must return within the system timeout: atomics and a few fast kernel calls
/// only.
unsafe extern "system" fn ll_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // The system invokes this with `code < 0` for events it wants passed on
    // unconditionally; `wparam`/`lparam` are then meaningless.
    if code < 0 {
        return call_next(code, wparam, lparam);
    }
    let Some(shared) = MANAGER.get() else {
        return LRESULT(0);
    };
    // While the manager is in the standard path (e.g. mid-rebind) the hook
    // must be inert and let everything through.
    if !shared.aggressive.load(Ordering::Relaxed) {
        return call_next(code, wparam, lparam);
    }
    // lparam points to a KBDLLHOOKSTRUCT owned by the system for the duration
    // of this callback; the cast is the documented access pattern.
    let ks = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

    let mut armed = shared.hook_armed.load(Ordering::Relaxed);
    let decision = decide_hook(
        &mut armed,
        ks.vkCode == shared.vk.load(Ordering::Relaxed),
        ks.flags.contains(LLKHF_UP),
        mods_match(shared),
    );
    shared.hook_armed.store(armed, Ordering::Relaxed);

    match decision {
        HookDecision::Pass => call_next(code, wparam, lparam),
        HookDecision::Swallow => LRESULT(1),
        HookDecision::SwallowAndFire => {
            let hwnd = HWND(shared.hwnd.load(Ordering::Relaxed) as *mut c_void);
            if !hwnd.is_invalid() {
                // FFI: hwnd is the live hidden window of this process.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_APP_TRIGGER, WPARAM(0), LPARAM(0));
                }
            }
            LRESULT(1)
        }
    }
}

/// What the hook should do with a keyboard event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookDecision {
    /// Forward the event to the next hook.
    Pass,
    /// Swallow the event (auto-repeat of a swallowed keydown, or the keyup of
    /// one).
    Swallow,
    /// Swallow and post the trigger — the arming keydown of the chord.
    SwallowAndFire,
}

/// Decides what the low-level hook does with a keystroke. Pure state
/// machine: `armed` is true while the hook is ready to fire the chord's next
/// keydown.
///
/// The keyup of the chord key re-arms the hook regardless of modifier state:
/// a rolled press (the key released before the modifier) must not leave the
/// hook disarmed, or the next chord press would be swallowed without firing —
/// indistinguishable from a broken hotkey.
fn decide_hook(armed: &mut bool, vk_is_chord: bool, is_up: bool, mods_match: bool) -> HookDecision {
    if !vk_is_chord {
        return HookDecision::Pass;
    }
    if is_up {
        *armed = true;
        // Swallow the keyup only when the modifiers still match — a matching
        // keyup always corresponds to a keydown we swallowed, and the target
        // app must not see a stray keyup. A mismatched keyup passes through;
        // re-arming is what matters.
        return if mods_match { HookDecision::Swallow } else { HookDecision::Pass };
    }
    if !mods_match {
        return HookDecision::Pass;
    }
    if std::mem::take(armed) {
        HookDecision::SwallowAndFire
    } else {
        // Auto-repeat of a chord keydown that already fired.
        HookDecision::Swallow
    }
}

/// Current modifier state must equal the chord's modifiers exactly, so
/// `Alt+Shift+V` never trips a plain `Alt+V` binding.
fn mods_match(shared: &Shared) -> bool {
    // FFI: GetAsyncKeyState with a valid VK code never faults; the sign bit of
    // the i16 result marks "down". Reflects state at the time of the keystroke
    // without blocking; it is the standard way to read modifiers from an LL hook.
    let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
    let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0;
    let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
    let win = unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) } < 0
        || unsafe { GetAsyncKeyState(VK_RWIN.0 as i32) } < 0;
    ctrl == shared.ctrl.load(Ordering::Relaxed)
        && alt == shared.alt.load(Ordering::Relaxed)
        && shift == shared.shift.load(Ordering::Relaxed)
        && win == shared.win.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(armed: &mut bool) -> HookDecision {
        decide_hook(armed, false, false, true)
    }

    #[test]
    fn non_chord_keys_always_pass() {
        let mut armed = true;
        assert_eq!(pass(&mut armed), HookDecision::Pass);
        assert!(armed, "non-chord keys must not touch the armed state");
        assert_eq!(decide_hook(&mut armed, false, true, true), HookDecision::Pass);
        assert!(armed);
    }

    #[test]
    fn arming_keydown_fires_once() {
        let mut armed = true;
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::SwallowAndFire);
        assert!(!armed);
        // Auto-repeat while the key is held: swallowed, never fires again.
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::Swallow);
        assert!(!armed);
    }

    #[test]
    fn mismatched_modifiers_pass_through() {
        let mut armed = true;
        assert_eq!(decide_hook(&mut armed, true, false, false), HookDecision::Pass);
        assert!(armed, "a passed-through keydown must not disarm the hook");
    }

    #[test]
    fn matching_keyup_swallows_and_rearms() {
        let mut armed = false;
        assert_eq!(decide_hook(&mut armed, true, true, true), HookDecision::Swallow);
        assert!(armed, "a keyup must re-arm the hook");
    }

    #[test]
    fn rolled_press_keyup_rearms_despite_mismatched_modifiers() {
        // The failure mode: Alt+V where the user releases Alt before V. The
        // V keyup arrives with the modifiers no longer matching, and the hook
        // must still re-arm — otherwise the next Alt+V is swallowed silently.
        let mut armed = false;
        assert_eq!(decide_hook(&mut armed, true, true, false), HookDecision::Pass);
        assert!(armed, "the rolled-press keyup must re-arm the hook");
    }

    #[test]
    fn full_press_cycle() {
        let mut armed = true;
        // Alt+V, normal release order: fire, swallow repeat, swallow keyup, re-arm.
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::SwallowAndFire);
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::Swallow);
        assert_eq!(decide_hook(&mut armed, true, true, true), HookDecision::Swallow);
        assert!(armed);
        // Rolled release order: fire, keyup with mismatched mods still re-arms.
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::SwallowAndFire);
        assert!(!armed);
        assert_eq!(decide_hook(&mut armed, true, true, false), HookDecision::Pass);
        assert!(armed, "the next press must fire again");
        assert_eq!(decide_hook(&mut armed, true, false, true), HookDecision::SwallowAndFire);
    }
}

fn call_next(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(shared) = MANAGER.get() else {
        return LRESULT(0);
    };
    let hook = HHOOK(shared.hook.load(Ordering::Relaxed) as *mut c_void);
    // FFI: forwarding our own hook handle is the documented pattern.
    unsafe { CallNextHookEx(Some(hook), code, wparam, lparam) }
}