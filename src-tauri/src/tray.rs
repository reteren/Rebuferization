//! Tray icon: Settings, and Enable/Disable. Left-click opens the popup.
//!
//! OWNER: worker W4.

use tauri::AppHandle;

use crate::error::AppResult;

pub fn install(_app: &AppHandle) -> AppResult<()> {
    todo!("W4")
}

/// Swaps to the muted icon variant while capture is disabled, and updates the
/// menu item label.
pub fn set_capture_enabled(_app: &AppHandle, _enabled: bool) -> AppResult<()> {
    todo!("W4")
}
