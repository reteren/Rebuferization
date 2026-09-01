//! Paste injection into the previously focused window.
//!
//! OWNER: worker W3.
//!
//! `show_popup` caches the foreground `HWND` here before it steals focus;
//! `hide_popup` and `send_paste` both restore that window. The cache lives
//! for the life of the process and is overwritten on every show, which is what
//! makes `send_paste()` signature-free.
//!
//! `send_paste` is deliberately defensive: the clipboard write has already
//! happened by the time it runs, so the worst it can do is skip the injection
//! (a nuisance) — but if it injects into the wrong window that is the
//! destructive case SPEC 5.4 warns about. It therefore verifies the cached
//! target is still alive and is what actually has focus, refuses targets that
//! run elevated (UIPI would silently drop the input anyway), and releases any
//! chord modifiers the user is still physically holding before sending Ctrl+V.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

use crate::error::AppResult;

/// HWND cached by `show_popup` before the popup takes focus; zero = none.
static TARGET: AtomicIsize = AtomicIsize::new(0);

pub fn set_target(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::SeqCst);
}

pub fn clear_target() {
    TARGET.store(0, Ordering::SeqCst);
}

pub fn target() -> HWND {
    HWND(TARGET.load(Ordering::SeqCst) as *mut c_void)
}

/// Brings the cached window back to the foreground. If the direct call is
/// refused (the classic foreground-lock), the `AttachThreadInput` workaround
/// attaches our input queue to the target's so it may take focus.
pub fn restore_foreground_window() {
    let hwnd = target();
    if hwnd.is_invalid() {
        return;
    }
    let ok = unsafe { SetForegroundWindow(hwnd) }; // FFI: hwnd is a live window handle validated above
    if !ok.as_bool() {
        let current = unsafe { GetCurrentThreadId() }; // FFI: takes no arguments
        let target_tid = unsafe { GetWindowThreadProcessId(hwnd, None) }; // FFI: out-param unused, NULL-safe
        if target_tid != 0 {
            unsafe {
                // Attach/restore around SetForegroundWindow is the documented
                // foreground-lock workaround; handles are thread ids, both valid.
                let _ = AttachThreadInput(current, target_tid, true);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(current, target_tid, false);
            }
        }
    }
}

/// Restores the cached foreground window, waits 30-50 ms for it to settle, then
/// sends `Ctrl+V` with `SendInput` — but only when the target provably still
/// exists and is what actually has focus. A missed auto-paste is a nuisance;
/// a paste into the wrong window is the destructive case SPEC 5.4 warns about,
/// so any doubt means skip and let the user paste by hand.
///
/// UIPI blocks the injection when the target runs elevated and we do not; the
/// clipboard write has already succeeded, so log it and return `Ok`, never an
/// error the user cannot act on.
pub fn send_paste() -> AppResult<()> {
    let hwnd = target();
    if hwnd.is_invalid() {
        tracing::warn!("send_paste: no foreground window was cached");
        return Ok(());
    }
    restore_foreground_window();
    // Give the restored window time to settle before the keystroke lands.
    std::thread::sleep(Duration::from_millis(40));

    // The cached HWND may be stale: the window it referred to can be closed
    // (or its HWND recycled for an unrelated window) while the popup was open.
    // SendInput does not address an HWND — it injects into whatever has focus
    // — so both checks are required before anything is sent.
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        tracing::warn!(
            "send_paste: the cached target window is gone; skipping auto-paste (clipboard is ready for manual paste)"
        );
        return Ok(());
    }
    if unsafe { GetForegroundWindow() } != hwnd {
        tracing::warn!(
            "send_paste: the cached target is not the foreground window; skipping auto-paste so it cannot land in the wrong window"
        );
        return Ok(());
    }

    match target_more_elevated(hwnd) {
        Some(true) => {
            tracing::warn!(
                "send_paste: the target runs elevated and we do not; UIPI will block injected input (clipboard is ready for manual paste)"
            );
            return Ok(());
        }
        Some(false) => {}
        None => {
            tracing::warn!(
                "send_paste: could not compare integrity levels with the target; injecting anyway"
            );
        }
    }

    let chord = crate::hotkey::active_chord();
    let held = chord_modifiers_held(&chord);
    let events = build_paste_events(held);
    // FFI: `events` owns the INPUT structs for the duration of the call.
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        // SendInput itself failed (it does not report UIPI filtering at the
        // target — that is checked separately above); the clipboard already
        // holds the content either way.
        tracing::warn!(
            "send_paste: SendInput was blocked (elevated target?) — clipboard is ready for manual paste"
        );
    }
    Ok(())
}

/// Which of the chord's modifiers the user is still physically holding.
/// These would otherwise ride along with the injected Ctrl+V, turning it into
/// e.g. Alt+Ctrl+V (which Word maps to Paste Special) or Win+Ctrl+V.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ModsHeld {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win_left: bool,
    win_right: bool,
}

/// Reads the physical modifier state with `GetAsyncKeyState` — checking, not
/// assuming: the user may already have released the modifiers.
fn chord_modifiers_held(chord: &crate::hotkey::Chord) -> ModsHeld {
    // FFI: GetAsyncKeyState with a valid VK code never faults; the sign bit of
    // the i16 result marks "down".
    ModsHeld {
        ctrl: chord.ctrl && unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0,
        alt: chord.alt && unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0,
        shift: chord.shift && unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0,
        win_left: chord.win && unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) } < 0,
        win_right: chord.win && unsafe { GetAsyncKeyState(VK_RWIN.0 as i32) } < 0,
    }
}

/// The `INPUT` sequence for one paste: a key-up for every chord modifier the
/// user is still holding (so the target sees a plain Ctrl+V), then Ctrl+V.
/// Deterministic release order: Ctrl, Alt, Shift, Win.
fn build_paste_events(held: ModsHeld) -> Vec<INPUT> {
    let mut events = Vec::with_capacity(8);
    if held.ctrl {
        events.push(key_input(VK_CONTROL, true));
    }
    if held.alt {
        events.push(key_input(VK_MENU, true));
    }
    if held.shift {
        events.push(key_input(VK_SHIFT, true));
    }
    if held.win_left {
        events.push(key_input(VK_LWIN, true));
    } else if held.win_right {
        events.push(key_input(VK_RWIN, true));
    }
    events.push(key_input(VK_CONTROL, false));
    events.push(key_input(VK_V, false));
    events.push(key_input(VK_V, true));
    events.push(key_input(VK_CONTROL, true));
    events
}

/// True when `hwnd`'s process runs at a higher integrity level than this one,
/// in which case UIPI will silently drop injected input. `None` when the
/// comparison cannot be made (the process is gone, access is denied, ...).
fn target_more_elevated(hwnd: HWND) -> Option<bool> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let ours = integrity_level(std::process::id())?;
    let theirs = integrity_level(pid)?;
    // Integrity SIDs sort in reverse privilege order: SYSTEM (0x800) >
    // HIGH (0x1000) > MEDIUM (0x2000) > LOW (0x4000). "More elevated" is a
    // numerically smaller level.
    Some(theirs < ours)
}

/// Mandatory integrity level of `pid` (the last SID sub-authority of its
/// token's mandatory label), or `None` if it cannot be read.
fn integrity_level(pid: u32) -> Option<u32> {
    // FFI: process/token handles are opened and closed around the query; the
    // token buffer is owned by this function for the duration of the calls.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
        let _ = CloseHandle(process);
        if !opened {
            return None;
        }
        // First call with a null buffer reports the required size; the second
        // fills it. The mandatory label is followed in memory by the SID
        // itself, which is why the size is queried rather than assumed.
        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut size);
        let mut buf = vec![0u8; (size as usize).max(std::mem::size_of::<TOKEN_MANDATORY_LABEL>())];
        let got = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr().cast()),
            buf.len() as u32,
            &mut size,
        );
        let _ = CloseHandle(token);
        if got.is_err() {
            return None;
        }
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        if label.Label.Sid.is_invalid() {
            return None;
        }
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(label.Label.Sid, count as u32 - 1))
    }
}

fn key_input(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, up: bool) -> INPUT {
    let mut flags = windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vk(input: &INPUT) -> u16 {
        // Sound: every INPUT these tests build is INPUT_KEYBOARD, so the `ki`
        // arm of the union is the initialised one.
        unsafe { input.Anonymous.ki.wVk.0 }
    }

    fn is_up(input: &INPUT) -> bool {
        // Sound: same as `vk` — the union's `ki` arm is the one written.
        unsafe { input.Anonymous.ki.dwFlags.contains(KEYEVENTF_KEYUP) }
    }

    fn assert_plain_ctrl_v(events: &[INPUT]) {
        assert_eq!(events.len(), 4);
        assert_eq!(vk(&events[0]), VK_CONTROL.0);
        assert!(!is_up(&events[0]));
        assert_eq!(vk(&events[1]), VK_V.0);
        assert!(!is_up(&events[1]));
        assert_eq!(vk(&events[2]), VK_V.0);
        assert!(is_up(&events[2]));
        assert_eq!(vk(&events[3]), VK_CONTROL.0);
        assert!(is_up(&events[3]));
    }

    #[test]
    fn nothing_held_is_a_plain_ctrl_v() {
        let events = build_paste_events(ModsHeld::default());
        assert_plain_ctrl_v(&events);
    }

    #[test]
    fn held_alt_is_released_before_ctrl_v() {
        // The Alt+V case: user still holding Alt when the card is clicked.
        let events = build_paste_events(ModsHeld {
            alt: true,
            ..Default::default()
        });
        assert_eq!(events.len(), 5);
        assert_eq!(vk(&events[0]), VK_MENU.0);
        assert!(is_up(&events[0]));
        assert_plain_ctrl_v(&events[1..]);
    }

    #[test]
    fn held_win_is_released_as_the_side_that_is_down() {
        let left = build_paste_events(ModsHeld {
            win_left: true,
            ..Default::default()
        });
        assert_eq!(vk(&left[0]), VK_LWIN.0);
        assert!(is_up(&left[0]));

        let right = build_paste_events(ModsHeld {
            win_right: true,
            ..Default::default()
        });
        assert_eq!(vk(&right[0]), VK_RWIN.0);
        assert!(is_up(&right[0]));
    }

    #[test]
    fn all_held_modifiers_release_in_order() {
        let events = build_paste_events(ModsHeld {
            ctrl: true,
            alt: true,
            shift: true,
            win_left: true,
            win_right: false,
        });
        assert_eq!(events.len(), 8);
        assert_eq!(vk(&events[0]), VK_CONTROL.0);
        assert_eq!(vk(&events[1]), VK_MENU.0);
        assert_eq!(vk(&events[2]), VK_SHIFT.0);
        assert_eq!(vk(&events[3]), VK_LWIN.0);
        for e in &events[0..4] {
            assert!(is_up(e), "releases must all be key-ups");
        }
        assert_plain_ctrl_v(&events[4..]);
    }

    #[test]
    fn win_left_wins_over_win_right() {
        let events = build_paste_events(ModsHeld {
            win_left: true,
            win_right: true,
            ..Default::default()
        });
        assert_eq!(vk(&events[0]), VK_LWIN.0);
        assert_eq!(events.len(), 5);
    }
}
