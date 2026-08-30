//! Popup placement, backdrop, focus handling, and paste injection.
//!
//! OWNER: worker W3.

pub mod paste;
pub mod position;
pub mod vibrancy;

use tauri::{AppHandle, WebviewWindow};

use crate::error::AppResult;

pub const POPUP_LABEL: &str = "popup";
pub const SETTINGS_LABEL: &str = "settings";

/// Caches the foreground `HWND`, positions the popup at the cursor clamped to
/// the work area, applies the backdrop, and shows it.
pub fn show_popup(_app: &AppHandle) -> AppResult<()> {
    todo!("W3")
}

/// Hides the popup and restores the previously cached foreground window.
pub fn hide_popup(_app: &AppHandle) -> AppResult<()> {
    todo!("W3")
}

pub fn show_settings(_app: &AppHandle) -> AppResult<()> {
    todo!("W3")
}

/// Applies acrylic/Mica on Windows 11, falling back to an opaque background on
/// Windows 10. Called once per window at startup.
pub fn apply_backdrop(_window: &WebviewWindow) -> AppResult<()> {
    todo!("W3")
}
