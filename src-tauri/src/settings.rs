//! `settings.json` — the shape is a shared contract (every module reads it, and
//! `src/lib/types.ts` mirrors it), so the structs below are fixed.
//!
//! OWNER: worker W4 owns loading, validation, atomic save, and hot reload.
//! Do not change field names or defaults without updating `types.ts` and
//! `docs/SPEC.md` §7 in the same change.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub hotkey: HotkeySettings,
    pub storage: StorageSettings,
    pub window: WindowSettings,
    pub behavior: BehaviorSettings,
    pub appearance: AppearanceSettings,
    pub privacy: PrivacySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HotkeySettings {
    pub binding: String,
    /// Low-level keyboard hook, needed for chords Windows already owns.
    pub aggressive_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StorageSettings {
    /// Absolute path; empty means "the default under %APPDATA%".
    pub path: String,
    /// 1..=30.
    pub retention_days: u32,
    pub max_item_bytes: u64,
    /// `None` = unlimited.
    pub max_store_bytes: Option<u64>,
    pub notify_when_full: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowSettings {
    /// `"percent"` or `"fixed"`.
    pub size_mode: String,
    pub percent_of_monitor: u32,
    pub fixed: FixedSize,
    /// 1..=5.
    pub zoom_step: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FixedSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BehaviorSettings {
    pub auto_paste: bool,
    pub paste_as_plain_text: bool,
    pub close_on_copy: bool,
    pub launch_on_startup: bool,
    pub silent_start: bool,
    pub capture_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceSettings {
    pub show_age: bool,
    /// `"off"` | `"small"` | `"medium"` | `"large"`.
    pub format_label_size: String,
    pub animate_gifs: bool,
    pub reduce_motion: bool,
    pub accent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    pub respect_clipboard_flags: bool,
    pub blocked_processes: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CURRENT_VERSION,
            hotkey: HotkeySettings::default(),
            storage: StorageSettings::default(),
            window: WindowSettings::default(),
            behavior: BehaviorSettings::default(),
            appearance: AppearanceSettings::default(),
            privacy: PrivacySettings::default(),
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        HotkeySettings { binding: "Alt+V".into(), aggressive_mode: false }
    }
}

impl Default for StorageSettings {
    fn default() -> Self {
        StorageSettings {
            path: String::new(),
            retention_days: 30,
            max_item_bytes: 256 * 1024 * 1024,
            max_store_bytes: None,
            notify_when_full: true,
        }
    }
}

impl Default for WindowSettings {
    fn default() -> Self {
        WindowSettings {
            size_mode: "percent".into(),
            percent_of_monitor: 40,
            fixed: FixedSize::default(),
            zoom_step: 3,
        }
    }
}

impl Default for FixedSize {
    fn default() -> Self {
        FixedSize { width: 1100, height: 700 }
    }
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        BehaviorSettings {
            auto_paste: false,
            paste_as_plain_text: false,
            close_on_copy: true,
            launch_on_startup: true,
            silent_start: true,
            capture_enabled: true,
        }
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        AppearanceSettings {
            show_age: true,
            format_label_size: "medium".into(),
            animate_gifs: true,
            reduce_motion: false,
            accent: "#7aa2ff".into(),
        }
    }
}

impl Default for PrivacySettings {
    fn default() -> Self {
        PrivacySettings {
            respect_clipboard_flags: true,
            blocked_processes: vec![
                "keepass.exe".into(),
                "keepassxc.exe".into(),
                "1password.exe".into(),
                "bitwarden.exe".into(),
                "lastpass.exe".into(),
                "dashlane.exe".into(),
                "protonpass.exe".into(),
            ],
        }
    }
}

impl Settings {
    /// Where the store lives, resolving the empty default to `%APPDATA%\Rebuffer`.
    pub fn store_root(&self) -> PathBuf {
        if self.storage.path.is_empty() {
            default_store_root()
        } else {
            PathBuf::from(&self.storage.path)
        }
    }
}

/// `%APPDATA%\Rebuffer`.
pub fn default_store_root() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Rebuffer")
}

/// Holds the current settings and the file watcher behind them.
pub struct SettingsStore;

impl SettingsStore {
    /// Reads `settings.json`, falling back to defaults for anything missing or
    /// out of range, and starts watching the file for external edits.
    pub fn load(_path: &Path) -> AppResult<SettingsStore> {
        todo!("W4")
    }

    pub fn get(&self) -> Settings {
        todo!("W4")
    }

    /// Merges a partial JSON patch, validates, saves atomically, and returns
    /// the result. Callers emit `settings-changed` afterwards.
    pub fn patch(&self, _patch: serde_json::Value) -> AppResult<Settings> {
        todo!("W4")
    }

    /// Clamps out-of-range values instead of rejecting the whole file, so one
    /// bad field never costs the user every other setting.
    pub fn validate(_s: &mut Settings) {
        todo!("W4")
    }
}
