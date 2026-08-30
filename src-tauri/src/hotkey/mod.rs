//! Global hotkey registration.
//!
//! OWNER: worker W3. Two paths: `RegisterHotKey` (default, cheap, reliable)
//! and an opt-in `WH_KEYBOARD_LL` hook for chords Windows already owns, such
//! as `Win+V`.

pub mod llhook;

use crate::error::AppResult;

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
    pub fn parse(_s: &str) -> AppResult<Chord> {
        todo!("W3")
    }

    /// Canonical display form, always in Ctrl+Alt+Shift+Win+Key order.
    pub fn to_display(&self) -> String {
        todo!("W3")
    }

    /// True for combinations Windows itself claims, which `RegisterHotKey`
    /// will refuse. The settings UI shows these as an inline explanation.
    pub fn is_system_reserved(&self) -> bool {
        todo!("W3")
    }
}

/// Holds whichever registration path is active. Dropping it unregisters.
pub struct HotkeyManager;

impl HotkeyManager {
    pub fn new(_on_trigger: Box<dyn Fn() + Send + Sync + 'static>) -> AppResult<HotkeyManager> {
        todo!("W3")
    }

    /// Swaps the binding at runtime, as the settings window does. `aggressive`
    /// selects the low-level hook path.
    pub fn rebind(&self, _chord: &Chord, _aggressive: bool) -> AppResult<()> {
        todo!("W3")
    }
}
