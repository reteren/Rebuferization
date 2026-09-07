//! `settings.json` — the shape is a shared contract (every module reads it, and
//! `src/lib/types.ts` mirrors it), so the structs below are fixed.
//!
//! OWNER: worker W4 owns loading, validation, atomic save, and hot reload.
//! Do not change field names or defaults without updating `types.ts` and
//! `docs/SPEC.md` §7 in the same change.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};

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
    /// An empty strip along the top of the popup that the window can be
    /// dragged by. It is added *on top of* the configured size, so it never
    /// costs the grid any room, which is why it is on by default: a window
    /// with no titlebar that cannot be moved is a window that is stuck.
    pub drag_bar: bool,
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
    /// Theme name: "darkblue" (built in) or one of the files under
    /// src/lib/styles/themes/. Unknown values fall back to "dark" rather than
    /// leaving the UI unstyled.
    pub theme: String,
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
    /// Whether a copied link may be looked up at the site that owns it, to
    /// give the card the page's real name instead of a bare hostname. On by
    /// the owner's decision, and still filed under privacy: it is the only
    /// thing in the app that sends anything anywhere.
    pub link_previews: bool,
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
        HotkeySettings {
            binding: "Alt+V".into(),
            aggressive_mode: false,
        }
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
            drag_bar: true,
        }
    }
}

impl Default for FixedSize {
    fn default() -> Self {
        FixedSize {
            width: 1100,
            height: 700,
        }
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
            theme: "darkblue".into(),
            show_age: true,
            format_label_size: "medium".into(),
            animate_gifs: true,
            reduce_motion: false,
            accent: String::new(),
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
            link_previews: true,
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
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Rebuffer")
}

// ---------------------------------------------------------------------------
// validation & sanitization
// ---------------------------------------------------------------------------

const SIZE_MODES: &[&str] = &["percent", "fixed"];
const LABEL_SIZES: &[&str] = &["off", "small", "medium", "large"];

/// `#RGB`, `#RRGGBB` or `#RRGGBBAA`.
fn is_valid_hex(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() == 4 || b.len() == 7 || b.len() == 9)
        && b[0] == b'#'
        && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

/// Post-deserialization clamp. Sanitization at the JSON level happens first
/// Mirrors THEMES in src/lib/types.ts. A name absent from both is not a theme.
const THEMES: [&str; 11] = [
    "darkblue",
    "black",
    "light",
    "grey",
    "skyblue",
    "dark-green",
    "dark-purple",
    "ember",
    "ocean",
    "wine",
    "paper",
];

/// (`sanitize_json`); this catches anything the type system let through.
pub fn validate(s: &mut Settings) {
    s.storage.retention_days = s.storage.retention_days.clamp(1, 30);
    s.window.zoom_step = s.window.zoom_step.clamp(1, 5);
    s.window.percent_of_monitor = s.window.percent_of_monitor.clamp(10, 100);
    // Empty means "follow the theme's accent", which is the default; only a
    // non-empty value is an override and has to be a real colour.
    if !s.appearance.accent.is_empty() && !is_valid_hex(&s.appearance.accent) {
        s.appearance.accent = AppearanceSettings::default().accent;
    }
    // An unknown theme name would leave the UI on whatever was loaded before.
    if !THEMES.contains(&s.appearance.theme.as_str()) {
        s.appearance.theme = AppearanceSettings::default().theme;
    }
    if !SIZE_MODES.contains(&s.window.size_mode.as_str()) {
        s.window.size_mode = WindowSettings::default().size_mode;
    }
    if !LABEL_SIZES.contains(&s.appearance.format_label_size.as_str()) {
        s.appearance.format_label_size = AppearanceSettings::default().format_label_size;
    }
}

/// Rewrites a `settings.json`-shaped `Value` so that a deserialization into
/// `Settings` can never fail and out-of-range values are clamped rather than
/// rejected: wrong-typed fields are dropped (their default applies), numbers
/// are clamped into their documented ranges, and enum-like strings fall back
/// to their default. One bad field never costs the user every other setting.
fn sanitize_json(v: &mut Value) {
    if !v.is_object() {
        *v = Value::Object(Map::new());
        return;
    }
    let root = v.as_object_mut().expect("checked above");

    if !matches!(root.get("version"), Some(Value::Number(_))) {
        root.remove("version");
    }

    sanitize_section(root, "hotkey", |o| {
        require_bool(o, "aggressiveMode");
        require_string(o, "binding");
    });

    sanitize_section(root, "storage", |o| {
        require_string(o, "path");
        clamp_number(o, "retentionDays", 1.0, 30.0);
        require_number(o, "maxItemBytes");
        require_number_or_null(o, "maxStoreBytes");
        require_bool(o, "notifyWhenFull");
    });

    sanitize_section(root, "window", |o| {
        require_one_of(
            o,
            "sizeMode",
            SIZE_MODES,
            &WindowSettings::default().size_mode,
        );
        clamp_number(o, "percentOfMonitor", 10.0, 100.0);
        clamp_number(o, "zoomStep", 1.0, 5.0);
        require_bool(o, "dragBar");
        sanitize_section(o, "fixed", |f| {
            require_number(f, "width");
            require_number(f, "height");
        });
    });

    sanitize_section(root, "behavior", |o| {
        for key in [
            "autoPaste",
            "pasteAsPlainText",
            "closeOnCopy",
            "launchOnStartup",
            "silentStart",
            "captureEnabled",
        ] {
            require_bool(o, key);
        }
    });

    sanitize_section(root, "appearance", |o| {
        require_bool(o, "showAge");
        require_one_of(
            o,
            "formatLabelSize",
            LABEL_SIZES,
            &AppearanceSettings::default().format_label_size,
        );
        require_bool(o, "animateGifs");
        require_bool(o, "reduceMotion");
        match o.get_mut("accent") {
            Some(Value::String(s)) if is_valid_hex(s) => {}
            _ => {
                o.insert(
                    "accent".into(),
                    Value::String(AppearanceSettings::default().accent),
                );
            }
        }
    });

    sanitize_section(root, "privacy", |o| {
        require_bool(o, "respectClipboardFlags");
        require_bool(o, "linkPreviews");
        match o.get_mut("blockedProcesses") {
            Some(Value::Array(items)) => items.retain(Value::is_string),
            _ => {
                o.remove("blockedProcesses");
            }
        }
    });
}

fn sanitize_section(
    obj: &mut Map<String, Value>,
    key: &str,
    f: impl FnOnce(&mut Map<String, Value>),
) {
    match obj.get_mut(key) {
        Some(Value::Object(o)) => f(o),
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

fn require_bool(obj: &mut Map<String, Value>, key: &str) {
    match obj.get_mut(key) {
        Some(Value::Bool(_)) => {}
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

fn require_string(obj: &mut Map<String, Value>, key: &str) {
    match obj.get_mut(key) {
        Some(Value::String(_)) => {}
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

fn require_number(obj: &mut Map<String, Value>, key: &str) {
    match obj.get_mut(key) {
        Some(Value::Number(_)) => {}
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

fn require_number_or_null(obj: &mut Map<String, Value>, key: &str) {
    match obj.get_mut(key) {
        Some(Value::Number(_)) | Some(Value::Null) => {}
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

/// Clamps a numeric field into `[lo, hi]`; a wrong-typed field is dropped so
/// the struct-level serde default applies.
fn clamp_number(obj: &mut Map<String, Value>, key: &str, lo: f64, hi: f64) {
    match obj.get_mut(key) {
        Some(Value::Number(n)) => {
            if let Some(f) = n.as_f64() {
                obj.insert(key.into(), Value::from(f.clamp(lo, hi) as u64));
            }
        }
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

/// Replaces an enum-like string field with the field's documented default if
/// it is not one of the allowed values, or drops it entirely if it is not a
/// string (the struct-level serde default then applies). The fallback must
/// never be "the first variant" — an invalid value must restore the default,
/// not silently pick whichever variant happens to head the match.
fn require_one_of(obj: &mut Map<String, Value>, key: &str, allowed: &[&str], fallback: &str) {
    match obj.get_mut(key) {
        Some(Value::String(s)) => {
            if !allowed.contains(&s.as_str()) {
                obj.insert(key.into(), Value::String(fallback.into()));
            }
        }
        Some(_) => {
            obj.remove(key);
        }
        None => {}
    }
}

/// Recursive JSON merge, roughly RFC 7396 without the null-means-delete rule:
/// objects merge key-wise, every other value replaces. `null` patches replace
/// the target (which is how `maxStoreBytes` is unset).
pub fn merge_into(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                match base.get_mut(&key) {
                    Some(existing) => merge_into(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

// ---------------------------------------------------------------------------
// storage
// ---------------------------------------------------------------------------

struct State {
    settings: Settings,
    /// mtime of the file as we last saw it; the watcher reloads when it differs.
    file_mtime: Option<SystemTime>,
}

/// Holds the current settings and the file watcher behind them.
pub struct SettingsStore {
    path: PathBuf,
    state: Arc<Mutex<State>>,
    /// Serializes writers so that two concurrent patches cannot interleave a
    /// merge with a write. It exists so `state` never has to be held across
    /// the write itself: `get` is called from the event-loop thread on every
    /// popup show, and a settings write does `create` + `write_all` +
    /// `sync_all` + `rename` and can touch the registry afterwards. Held
    /// across all of that, `state` would stall the whole UI on a slow disk.
    /// No reader ever takes this lock.
    writing: Mutex<()>,
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Writes `settings.json` atomically: temp file in the same directory, fsync,
/// then rename over the target (which replaces on Windows).
///
/// Returns the mtime of the file as it stood immediately after the rename.
/// Reading it later, after the caller has taken a lock, would let an external
/// edit land in between and be recorded as our own write — the watcher would
/// then treat that edit as already seen and never load it.
fn write_atomic(path: &Path, settings: &Settings) -> AppResult<Option<SystemTime>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(settings)?;
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    Ok(file_mtime(path))
}

impl SettingsStore {
    /// Reads `settings.json`, falling back to defaults for anything missing or
    /// out of range, and starts watching the file for external edits.
    ///
    /// A missing file is not an error: the defaults are written so the user
    /// can inspect them. A corrupt file is also not an error: defaults are
    /// used in memory but the file is left untouched until the next `patch`.
    pub fn load(path: &Path) -> AppResult<SettingsStore> {
        let (settings, mtime) = match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(mut value) => {
                    sanitize_json(&mut value);
                    match serde_json::from_value::<Settings>(value) {
                        Ok(mut s) => {
                            validate(&mut s);
                            (s, file_mtime(path))
                        }
                        Err(e) => {
                            tracing::warn!("settings.json is unreadable, using defaults: {e}");
                            (Settings::default(), None)
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("settings.json is not valid JSON, using defaults: {e}");
                    (Settings::default(), None)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let s = Settings::default();
                let mtime = write_atomic(path, &s)?;
                (s, mtime)
            }
            Err(e) => return Err(e.into()),
        };

        let state = Arc::new(Mutex::new(State {
            settings,
            file_mtime: mtime,
        }));
        let store = SettingsStore {
            path: path.to_path_buf(),
            state: state.clone(),
            writing: Mutex::new(()),
        };

        // Hot reload: a plain mtime poll, deliberately not a filesystem-watcher
        // crate — this is a settings file, not a build system. The `file_mtime`
        // we record on every write of our own is what lets us skip reloads of
        // our own edits.
        let watch_path = path.to_path_buf();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            let current = file_mtime(&watch_path);
            let known = state.lock().file_mtime;
            if current == known {
                continue;
            }
            // Read and parse with the lock released. Held across the read, a
            // settings file on a busy disk would stall every `get`, and `get`
            // runs on the event-loop thread on each popup show.
            let reloaded = fs::read(&watch_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|mut value| {
                    sanitize_json(&mut value);
                    serde_json::from_value::<Settings>(value).ok()
                })
                .map(|mut s| {
                    validate(&mut s);
                    s
                });
            let mut st = state.lock();
            // A `patch` may have landed while we were reading; it wrote the
            // newer state and the newer mtime, and this reload would put the
            // older file contents back over it.
            if st.file_mtime != known {
                continue;
            }
            if let Some(s) = reloaded {
                st.settings = s;
            }
            st.file_mtime = current;
        });

        Ok(store)
    }

    pub fn get(&self) -> Settings {
        self.state.lock().settings.clone()
    }

    /// Merges a partial JSON patch, validates, saves atomically, and returns
    /// the result. Callers emit `settings-changed` afterwards.
    pub fn patch(&self, patch: Value) -> AppResult<Settings> {
        if !patch.is_object() {
            return Err(AppError::Other(
                "settings patch must be a JSON object".into(),
            ));
        }
        // Writers are serialized by their own lock so that the state lock is
        // only ever held for a copy; see the `writing` field.
        let _writing = self.writing.lock();
        let current = self.state.lock().settings.clone();

        let mut merged = serde_json::to_value(&current)?;
        merge_into(&mut merged, patch);
        sanitize_json(&mut merged);
        let mut next: Settings = serde_json::from_value(merged)?;
        validate(&mut next);

        let behavior_changed = current.behavior.launch_on_startup
            != next.behavior.launch_on_startup
            || current.behavior.silent_start != next.behavior.silent_start;

        // The file first, still with no lock held: a write that fails must
        // leave the in-memory settings exactly as they were.
        let written_mtime = write_atomic(&self.path, &next)?;
        {
            let mut st = self.state.lock();
            st.settings = next.clone();
            st.file_mtime = written_mtime;
        }

        if behavior_changed {
            apply_autostart(&next.behavior);
        }
        Ok(next)
    }

    /// Clamps out-of-range values instead of rejecting the whole file, so one
    /// bad field never costs the user every other setting.
    pub fn validate(s: &mut Settings) {
        validate(s)
    }
}

// ---------------------------------------------------------------------------
// autostart wiring
// ---------------------------------------------------------------------------

type AutostartHook = Arc<dyn Fn(&BehaviorSettings) + Send + Sync>;

/// Registered once at startup (by the tray installer, which holds the
/// `AppHandle`); `patch` calls it when `launchOnStartup` or `silentStart`
/// changes so the HKCU Run key follows the setting.
static AUTOSTART_HOOK: once_cell::sync::Lazy<Mutex<Option<AutostartHook>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

pub fn set_autostart_hook<F>(hook: F)
where
    F: Fn(&BehaviorSettings) + Send + Sync + 'static,
{
    *AUTOSTART_HOOK.lock() = Some(Arc::new(hook));
}

/// Applies the current startup behavior through the registered hook (if any).
pub fn apply_autostart(behavior: &BehaviorSettings) {
    // Clone the handle out and drop the lock before running it. The hook
    // enables or disables autostart and rewrites an HKCU value, and holding a
    // global lock across registry work is how a slow call turns into a stalled
    // caller.
    let hook = AUTOSTART_HOOK.lock().clone();
    if let Some(hook) = hook {
        hook(behavior);
    }
}

/// Reads just the behavior section from disk, sanitized, for callers that
/// need the startup state before the store is managed (the tray installer).
pub fn peek_behavior() -> BehaviorSettings {
    let path = default_store_root().join("settings.json");
    let value = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let Some(mut value) = value else {
        return BehaviorSettings::default();
    };
    sanitize_json(&mut value);
    serde_json::from_value::<Settings>(value)
        .map(|mut s| {
            validate(&mut s);
            s.behavior
        })
        .unwrap_or_default()
}

/// The `tauri-plugin-autostart` run key always carries `--silent` because its
/// args are fixed at plugin init; when `silentStart` is off this rewrites the
/// value without it. Uses the registry directly (the `Win32_System_Registry`
/// feature is enabled transitively by the autostart plugin's `winreg`).
pub fn rewrite_run_value(app_name: &str, silent: bool) -> AppResult<()> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    };

    let mut value = format!("\"{}\"", std::env::current_exe()?.display());
    if silent {
        value.push_str(" --silent");
    }

    let name = wide(app_name);
    let data: Vec<u8> = wide(&value)
        .into_iter()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let mut hkey = HKEY::default();
    unsafe {
        let err = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if err.0 != 0 {
            return Err(AppError::Win(format!(
                "RegOpenKeyExW(Run) failed: {}",
                err.0
            )));
        }
        let err = RegSetValueExW(hkey, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data));
        if err.0 != 0 {
            let _ = RegCloseKey(hkey);
            return Err(AppError::Win(format!("RegSetValueExW failed: {}", err.0)));
        }
        let _ = RegCloseKey(hkey);
    }
    Ok(())
}

/// NUL-terminated UTF-16, for `PCWSTR` parameters.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Windows clipboard history (Win+V)
// ---------------------------------------------------------------------------
//
// This is OS state, deliberately NOT in settings.json: it lives in HKCU and the
// Settings UI reflects the live system value rather than a stored preference,
// so settings.json never shadows a change the user made in Windows Settings.
// Mirror of the installer's `clipboard-history.nsh`, but without its foot-gun:
// the installer compares the read value numerically, so an ABSENT value is
// coerced to 0 and treated as "was enabled"; here absence is detected with the
// registry API's own not-found error.

/// Maps a raw `EnableClipboardHistory` DWORD to "enabled". ABSENT means enabled
/// (the Windows default), 0 means disabled, and any other value means enabled.
fn clipboard_history_enabled_from_dword(value: Option<u32>) -> bool {
    !matches!(value, Some(0))
}

/// Reads the current state of Windows clipboard history for this user:
/// `HKCU\Software\Microsoft\Clipboard\EnableClipboardHistory` is a REG_DWORD
/// where 1 or ABSENT both mean enabled and 0 means disabled.
pub fn clipboard_history_enabled() -> AppResult<bool> {
    use windows::core::w;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
        REG_VALUE_TYPE,
    };

    let mut hkey = HKEY::default();
    // SAFETY: hkey is a stack buffer the API fills; all parameters are plain values.
    let err = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Clipboard"),
            None,
            KEY_READ,
            &mut hkey,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(AppError::Win(format!(
            "RegOpenKeyExW(Software\\Microsoft\\Clipboard) failed: {}",
            err.0
        )));
    }

    let mut value: u32 = 0;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let mut kind: REG_VALUE_TYPE = REG_DWORD;
    // SAFETY: value/size/kind are stack buffers of the right width for a DWORD
    // query; the key handle is live and open for KEY_READ.
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            w!("EnableClipboardHistory"),
            None,
            Some(&mut kind),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    // The handle must close on every path.
    // SAFETY: the key handle is still valid.
    unsafe {
        let _ = RegCloseKey(hkey);
    }

    if err == ERROR_FILE_NOT_FOUND {
        // ABSENT = enabled, per the Windows default.
        return Ok(true);
    }
    if err != ERROR_SUCCESS {
        return Err(AppError::Win(format!(
            "RegQueryValueExW(EnableClipboardHistory) failed: {}",
            err.0
        )));
    }
    // A non-DWORD value (or a truncated read) is treated as enabled rather than
    // guessing at a disabled state from garbage bytes — the mapping rule is
    // "only an explicit 0 disables".
    if kind != REG_DWORD || size != std::mem::size_of::<u32>() as u32 {
        return Ok(true);
    }
    Ok(clipboard_history_enabled_from_dword(Some(value)))
}

/// Sets Windows clipboard history for this user. Always writes an explicit
/// DWORD (1 = enabled, 0 = disabled) rather than deleting the value, so the
/// state stays explicit and matches what the Windows Settings UI writes.
pub fn set_clipboard_history_enabled(enabled: bool) -> AppResult<()> {
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_DWORD,
    };

    let mut hkey = HKEY::default();
    // SAFETY: hkey is a stack buffer the API fills; all parameters are plain values.
    let err = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Clipboard"),
            None,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if err != ERROR_SUCCESS {
        return Err(AppError::Win(format!(
            "RegOpenKeyExW(Software\\Microsoft\\Clipboard) failed: {}",
            err.0
        )));
    }

    let value: u32 = if enabled { 1 } else { 0 };
    let bytes = value.to_le_bytes();
    // SAFETY: the key handle is live and open for KEY_SET_VALUE; the slice is a
    // 4-byte DWORD little-endian, which is what REG_DWORD stores.
    let err = unsafe {
        RegSetValueExW(
            hkey,
            w!("EnableClipboardHistory"),
            None,
            REG_DWORD,
            Some(&bytes),
        )
    };
    // SAFETY: the key handle is still valid.
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if err != ERROR_SUCCESS {
        return Err(AppError::Win(format!(
            "RegSetValueExW(EnableClipboardHistory) failed: {}",
            err.0
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sanitized(v: Value) -> Value {
        let mut v = v;
        sanitize_json(&mut v);
        v
    }

    fn as_settings(v: Value) -> Settings {
        let mut s: Settings = serde_json::from_value(v).expect("sanitized json must deserialize");
        validate(&mut s);
        s
    }

    #[test]
    fn clamps_retention_days() {
        let v = sanitized(json!({ "storage": { "retentionDays": 45 } }));
        let s = as_settings(v);
        assert_eq!(s.storage.retention_days, 30);
        let v = sanitized(json!({ "storage": { "retentionDays": 0 } }));
        assert_eq!(as_settings(v).storage.retention_days, 1);
    }

    #[test]
    fn clamps_zoom_step() {
        let v = sanitized(json!({ "window": { "zoomStep": 9 } }));
        assert_eq!(as_settings(v).window.zoom_step, 5);
        let v = sanitized(json!({ "window": { "zoomStep": 0 } }));
        assert_eq!(as_settings(v).window.zoom_step, 1);
    }

    #[test]
    fn clamps_percent_of_monitor() {
        let v = sanitized(json!({ "window": { "percentOfMonitor": 250 } }));
        assert_eq!(as_settings(v).window.percent_of_monitor, 100);
        let v = sanitized(json!({ "window": { "percentOfMonitor": 3 } }));
        assert_eq!(as_settings(v).window.percent_of_monitor, 10);
    }

    #[test]
    fn valid_range_values_pass_through() {
        let v = sanitized(json!({
            "storage": { "retentionDays": 17 },
            "window": { "zoomStep": 3, "percentOfMonitor": 42 }
        }));
        let s = as_settings(v);
        assert_eq!(s.storage.retention_days, 17);
        assert_eq!(s.window.zoom_step, 3);
        assert_eq!(s.window.percent_of_monitor, 42);
    }

    /// The default accent is now EMPTY, meaning "follow the theme". A bad
    /// value must fall back to that rather than to a hardcoded blue, or a
    /// typo in settings.json would pin every theme to the dark theme's accent.
    #[test]
    fn accent_falls_back_to_following_the_theme() {
        let v = sanitized(json!({ "appearance": { "accent": "hotpink" } }));
        assert_eq!(as_settings(v).appearance.accent, "");
        let v = sanitized(json!({ "appearance": { "accent": 42 } }));
        assert_eq!(as_settings(v).appearance.accent, "");
    }

    /// An explicit colour is still honoured — the setting is an override, not
    /// a dead control.
    #[test]
    fn explicit_accent_survives_validation() {
        let v = sanitized(json!({ "appearance": { "accent": "#ff8800" } }));
        assert_eq!(as_settings(v).appearance.accent, "#ff8800");
    }

    #[test]
    fn unknown_theme_falls_back_to_the_built_in_one() {
        let v = sanitized(json!({ "appearance": { "theme": "chartreuse" } }));
        assert_eq!(as_settings(v).appearance.theme, "darkblue");
        let v = sanitized(json!({ "appearance": { "theme": "dark-green" } }));
        assert_eq!(as_settings(v).appearance.theme, "dark-green");
    }

    #[test]
    fn accent_accepts_valid_hex_forms() {
        for hex in ["#fff", "#7aa2ff", "#7aa2ffcc"] {
            let v = sanitized(json!({ "appearance": { "accent": hex } }));
            assert_eq!(as_settings(v).appearance.accent, hex);
        }
    }

    #[test]
    fn wrong_typed_field_falls_back_not_dies() {
        let v =
            sanitized(json!({ "storage": { "retentionDays": "thirty", "notifyWhenFull": false } }));
        let s = as_settings(v);
        assert_eq!(s.storage.retention_days, 30);
        assert!(
            !s.storage.notify_when_full,
            "healthy sibling field must survive"
        );
    }

    #[test]
    fn one_bad_field_keeps_everything_else() {
        let v = sanitized(json!({
            "hotkey": { "binding": "Alt+V", "aggressiveMode": "yes" },
            "storage": { "retentionDays": 99, "path": 12, "maxStoreBytes": "lots" },
            "behavior": { "launchOnStartup": true, "silentStart": true }
        }));
        let s = as_settings(v);
        assert_eq!(s.hotkey.binding, "Alt+V");
        assert!(!s.hotkey.aggressive_mode);
        assert_eq!(s.storage.retention_days, 30);
        assert!(s.storage.path.is_empty());
        assert_eq!(s.storage.max_store_bytes, None);
        assert!(s.behavior.launch_on_startup);
        assert!(s.behavior.silent_start);
    }

    #[test]
    fn bad_section_type_is_dropped() {
        let v = sanitized(json!({ "appearance": "pretty" }));
        let s = as_settings(v);
        assert!(s.appearance.show_age);
        assert_eq!(s.appearance.format_label_size, "medium");
    }

    #[test]
    fn enum_strings_fall_back() {
        let v = sanitized(json!({ "window": { "sizeMode": "gigantic" } }));
        assert_eq!(as_settings(v).window.size_mode, "percent");
        let v = sanitized(json!({ "appearance": { "formatLabelSize": "huge" } }));
        assert_eq!(as_settings(v).appearance.format_label_size, "medium");
    }

    #[test]
    fn non_object_root_becomes_defaults() {
        let v = sanitized(json!([1, 2, 3]));
        let s: Settings = serde_json::from_value(v).expect("must deserialize");
        assert_eq!(s.storage.retention_days, 30);
        assert_eq!(s.hotkey.binding, "Alt+V");
    }

    #[test]
    fn blocked_processes_keeps_only_strings() {
        let v = sanitized(json!({ "privacy": { "blockedProcesses": ["keepass.exe", 7, null] } }));
        assert_eq!(
            as_settings(v).privacy.blocked_processes,
            vec!["keepass.exe"]
        );
        let v = sanitized(json!({ "privacy": { "blockedProcesses": "keepass.exe" } }));
        assert_eq!(
            as_settings(v).privacy.blocked_processes,
            PrivacySettings::default().blocked_processes
        );
    }

    #[test]
    fn merge_is_recursive_and_replaces_arrays() {
        let mut base = json!({
            "behavior": { "launchOnStartup": true, "silentStart": true },
            "appearance": { "accent": "#7aa2ff" },
            "privacy": { "blockedProcesses": ["keepass.exe"] }
        });
        let patch = json!({
            "behavior": { "silentStart": false },
            "privacy": { "blockedProcesses": ["bitwarden.exe"] },
            "storage": { "maxStoreBytes": null }
        });
        merge_into(&mut base, patch);
        assert_eq!(base["behavior"]["launchOnStartup"], true);
        assert_eq!(base["behavior"]["silentStart"], false);
        assert_eq!(base["appearance"]["accent"], "#7aa2ff");
        assert_eq!(
            base["privacy"]["blockedProcesses"],
            json!(["bitwarden.exe"])
        );
        assert_eq!(base["storage"]["maxStoreBytes"], Value::Null);
    }

    #[test]
    fn merge_replaces_non_object_values() {
        let mut base = json!({ "version": 1 });
        merge_into(&mut base, json!({ "version": 2 }));
        assert_eq!(base["version"], 2);
    }

    #[test]
    fn patch_round_trip_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(&path).expect("load");
        let patched = store
            .patch(json!({ "window": { "zoomStep": 9 }, "appearance": { "accent": "#00ff00" } }))
            .expect("patch");
        assert_eq!(patched.window.zoom_step, 5, "patch is clamped too");
        assert_eq!(patched.appearance.accent, "#00ff00");

        let on_disk: Value =
            serde_json::from_slice(&fs::read(&path).expect("file exists")).expect("valid json");
        assert_eq!(on_disk["window"]["zoomStep"], 5);
        assert_eq!(on_disk["appearance"]["accent"], "#00ff00");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no temp file left behind"
        );
    }

    /// The one setting that lets the app talk to anyone. On by default, so an
    /// upgrade opts in — but a user who switches it off must have it stay off
    /// across a restart, and junk must not read as consent.
    #[test]
    fn link_previews_default_on_and_a_refusal_sticks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(&path).expect("load");
        assert!(store.get().privacy.link_previews, "on out of the box");

        // Switching it off is the one answer that must never be quietly undone.
        let patched = store
            .patch(json!({ "privacy": { "linkPreviews": false } }))
            .expect("patch");
        assert!(!patched.privacy.link_previews);
        let on_disk: Value =
            serde_json::from_slice(&fs::read(&path).expect("file exists")).expect("valid json");
        assert_eq!(on_disk["privacy"]["linkPreviews"], false);
        let reopened = SettingsStore::load(&path).expect("reload");
        assert!(
            !reopened.get().privacy.link_previews,
            "a refusal survives a restart"
        );

        let old_path = dir.path().join("old.json");
        fs::write(
            &old_path,
            br#"{"version":1,"privacy":{"respectClipboardFlags":false}}"#,
        )
        .expect("write");
        let old = SettingsStore::load(&old_path).expect("load");
        assert!(old.get().privacy.link_previews);
        assert!(!old.get().privacy.respect_clipboard_flags);

        let bad_path = dir.path().join("bad.json");
        fs::write(
            &bad_path,
            br#"{"version":1,"privacy":{"linkPreviews":"sure","respectClipboardFlags":false}}"#,
        )
        .expect("write");
        let bad = SettingsStore::load(&bad_path).expect("load");
        assert!(bad.get().privacy.link_previews, "junk is not a refusal");
        assert!(
            !bad.get().privacy.respect_clipboard_flags,
            "one bad field costs no other"
        );
    }

    #[test]
    fn the_drag_bar_is_on_by_default_and_can_be_switched_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(&path).expect("load");
        assert!(store.get().window.drag_bar, "on unless the user says no");

        let patched = store
            .patch(json!({ "window": { "dragBar": false } }))
            .expect("patch");
        assert!(!patched.window.drag_bar);
        let on_disk: Value =
            serde_json::from_slice(&fs::read(&path).expect("file exists")).expect("valid json");
        assert_eq!(on_disk["window"]["dragBar"], false);

        // A settings.json written before the setting existed must still load,
        // and picks up the default like any other missing field.
        let old_path = dir.path().join("old.json");
        fs::write(
            &old_path,
            br#"{"version":1,"window":{"sizeMode":"fixed","zoomStep":2}}"#,
        )
        .expect("write");
        let old = SettingsStore::load(&old_path).expect("load");
        assert!(old.get().window.drag_bar);
        assert_eq!(old.get().window.size_mode, "fixed");

        // And a nonsense value falls back rather than costing the whole section.
        let bad_path = dir.path().join("bad.json");
        fs::write(
            &bad_path,
            br#"{"version":1,"window":{"dragBar":"yes please","zoomStep":4}}"#,
        )
        .expect("write");
        let bad = SettingsStore::load(&bad_path).expect("load");
        assert!(bad.get().window.drag_bar);
        assert_eq!(
            bad.get().window.zoom_step,
            4,
            "one bad field costs no other"
        );
    }

    #[test]
    fn missing_file_writes_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(&path).expect("load");
        assert_eq!(store.get().storage.retention_days, 30);
        assert!(path.exists(), "defaults must be materialized");
    }

    #[test]
    fn patch_must_be_an_object() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(&path).expect("load");
        assert!(store.patch(json!(42)).is_err());
    }

    #[test]
    fn validate_clamps_struct_directly() {
        let mut s = Settings::default();
        s.storage.retention_days = 0;
        s.window.zoom_step = 42;
        s.window.percent_of_monitor = 1;
        s.appearance.accent = "red".into();
        SettingsStore::validate(&mut s);
        assert_eq!(s.storage.retention_days, 1);
        assert_eq!(s.window.zoom_step, 5);
        assert_eq!(s.window.percent_of_monitor, 10);
        assert_eq!(s.appearance.accent, "");
    }

    // -----------------------------------------------------------------------
    // Windows clipboard history state mapping. This is where the installer
    // went wrong (numeric comparison coerced ABSENT to 0), so the mapping is
    // pinned by tests: only an explicit 0 disables; ABSENT and anything else
    // mean enabled.
    // -----------------------------------------------------------------------

    #[test]
    fn clipboard_history_absent_means_enabled() {
        assert!(clipboard_history_enabled_from_dword(None));
    }

    #[test]
    fn clipboard_history_one_means_enabled() {
        assert!(clipboard_history_enabled_from_dword(Some(1)));
    }

    #[test]
    fn clipboard_history_zero_means_disabled() {
        assert!(!clipboard_history_enabled_from_dword(Some(0)));
    }

    #[test]
    fn clipboard_history_any_other_value_means_enabled() {
        for v in [2u32, 3, 42, 4_000_000_000, u32::MAX] {
            assert!(
                clipboard_history_enabled_from_dword(Some(v)),
                "value {v} must map to enabled"
            );
        }
    }
}
