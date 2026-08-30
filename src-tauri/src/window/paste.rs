//! Paste injection into the previously focused window.
//!
//! OWNER: worker W3.
//!
//! `show_popup` caches the foreground `HWND` here before it steals focus;
//! `hide_popup` and `send_paste` both restore that window. The cache lives
//! for the life of the process and is overwritten on every show, which is what
//! makes `send_paste()` signature-free.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, SetForegroundWindow,
};

use crate::error::AppResult;

/// HWND cached by `show_popup` before the popup takes focus; zero = none.
static TARGET: AtomicIsize = AtomicIsize::new(0);

pub fn set_target(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::SeqCst);
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
/// sends `Ctrl+V` with `SendInput`.
///
/// UIPI blocks this when the target runs elevated and we do not; the clipboard
/// write has already succeeded, so log it and return `Ok`, never an error the
/// user cannot act on.
pub fn send_paste() -> AppResult<()> {
    let hwnd = target();
    if hwnd.is_invalid() {
        tracing::warn!("send_paste: no foreground window was cached");
        return Ok(());
    }
    restore_foreground_window();
    // Give the restored window time to settle before the keystroke lands.
    std::thread::sleep(Duration::from_millis(40));

    let events = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];
    // FFI: `events` owns the INPUT structs for the duration of the call.
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        // Either the target is elevated (UIPI) or the window vanished; the
        // clipboard already holds the content either way.
        tracing::warn!(
            "send_paste: SendInput was blocked (elevated target?) — clipboard is ready for manual paste"
        );
    }
    Ok(())
}

fn key_input(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, up: bool) -> INPUT {
    let mut flags = windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}