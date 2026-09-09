//! Clipboard capture: the hidden listener window, format decoders, the privacy
//! filter, and the writer used for paste-back.
//!
//! OWNER: worker W2. Nothing outside `clipboard/` calls a Win32 clipboard API.

pub mod classify;
pub mod decode;
pub mod listener;
pub mod privacy;
pub mod writer;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_DESTROY};

use crate::capture::Capture;
use crate::error::AppResult;
use crate::store::Store;

/// Owns the hidden `HWND` and the clipboard-format listener registration.
/// Dropping it unregisters the listener and destroys the window.
pub struct ClipboardWatcher {
    #[allow(dead_code)]
    pub(crate) store: Arc<Store>,
    pub(crate) enabled: Arc<AtomicBool>,
    pub(crate) hwnd_raw: isize,
    pub(crate) join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl ClipboardWatcher {
    /// Spawns the message-loop thread, creates the hidden window, and calls
    /// `AddClipboardFormatListener`. Returns once the window exists.
    pub fn start(store: Arc<Store>, on_item: OnItem) -> AppResult<ClipboardWatcher> {
        listener::spawn_listener(store, on_item)
    }

    /// The tray Enable/Disable toggle. Disabled stops capture but keeps the
    /// window, the hotkey, and the existing history alive.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
}

impl Drop for ClipboardWatcher {
    fn drop(&mut self) {
        if self.hwnd_raw != 0 {
            unsafe {
                // Sound: hwnd_raw was a valid HWND returned by CreateWindowExW; PostMessageW sends WM_DESTROY asynchronously to trigger window destruction and loop exit.
                let hwnd = HWND(self.hwnd_raw as *mut core::ffi::c_void);
                let _ = PostMessageW(Some(hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(handle) = self.join_handle.lock().take() {
            let _ = handle.join();
        }
    }
}

/// Called on the listener thread after a capture is committed, so the UI can
/// emit `item-added`.
pub type OnItem = Box<dyn Fn(crate::model::ItemDto) + Send + Sync + 'static>;

/// Reads the clipboard right now and decodes it, without persisting. Used by
/// the debug path and by tests.
pub fn read_current(source_app: Option<String>) -> AppResult<Option<Capture>> {
    let max_bytes = 256 * 1024 * 1024; // 256 MB default
                                       // Copy under the lock, decode after it. Even off the hot path this must not
                                       // hold the clipboard while it works; see `decode::RawClipboard`.
    let raw = {
        let _guard = writer::ClipboardGuard::open_with_retry(None)?;
        decode::grab_clipboard(max_bytes)?
    };
    match raw {
        Some(raw) => decode::build_capture(raw, max_bytes, source_app),
        None => Ok(None),
    }
}
