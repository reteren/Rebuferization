//! Clipboard capture: the hidden listener window, format decoders, the privacy
//! filter, and the writer used for paste-back.
//!
//! OWNER: worker W2. Nothing outside `clipboard/` calls a Win32 clipboard API.

pub mod classify;
pub mod decode;
pub mod listener;
pub mod privacy;
pub mod writer;

use std::sync::Arc;

use crate::capture::Capture;
use crate::error::AppResult;
use crate::store::Store;

/// Owns the hidden `HWND` and the clipboard-format listener registration.
/// Dropping it unregisters the listener and destroys the window.
pub struct ClipboardWatcher {
    #[allow(dead_code)]
    store: Arc<Store>,
}

impl ClipboardWatcher {
    /// Spawns the message-loop thread, creates the hidden window, and calls
    /// `AddClipboardFormatListener`. Returns once the window exists.
    pub fn start(_store: Arc<Store>, _on_item: OnItem) -> AppResult<ClipboardWatcher> {
        todo!("W2")
    }

    /// The tray Enable/Disable toggle. Disabled stops capture but keeps the
    /// window, the hotkey, and the existing history alive.
    pub fn set_enabled(&self, _enabled: bool) {
        todo!("W2")
    }

    pub fn is_enabled(&self) -> bool {
        todo!("W2")
    }
}

/// Called on the listener thread after a capture is committed, so the UI can
/// emit `item-added`.
pub type OnItem = Box<dyn Fn(crate::model::ItemDto) + Send + Sync + 'static>;

/// Reads the clipboard right now and decodes it, without persisting. Used by
/// the debug path and by tests.
pub fn read_current(_source_app: Option<String>) -> AppResult<Option<Capture>> {
    todo!("W2")
}
