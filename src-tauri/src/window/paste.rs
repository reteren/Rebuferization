//! Paste injection into the previously focused window.
//!
//! OWNER: worker W3.

use crate::error::AppResult;

/// Restores the cached foreground window, waits 30-50 ms for it to settle, then
/// sends `Ctrl+V` with `SendInput`.
///
/// UIPI blocks this when the target runs elevated and we do not; the clipboard
/// write has already succeeded, so log it and return `Ok`, never an error the
/// user cannot act on.
pub fn send_paste() -> AppResult<()> {
    todo!("W3")
}
