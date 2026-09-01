//! Rebuffer — persistent clipboard history for Windows.
//!
//! Three long-lived pieces share one process: the message-loop thread that owns
//! the hidden clipboard-listener window, the store, and two pre-created
//! WebView2 windows. See `docs/SPEC.md` §1.
//!
//! OWNER: the coordinator. Workers add to their own modules, not here.

pub mod capture;
pub mod clipboard;
pub mod commands;
pub mod error;
pub mod hotkey;
pub mod logging;
pub mod model;
pub mod settings;
pub mod shell;
pub mod store;
pub mod tray;
pub mod window;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tauri::Manager;

use crate::clipboard::ClipboardWatcher;
use crate::settings::SettingsStore;
use crate::store::Store;

/// The item currently sitting on the clipboard, or 0 for "none we know of".
///
/// Every clipboard change is either a capture we just stored or a write we made
/// ourselves, so the id is known at both moments; there is nowhere else it can
/// come from. It is deliberately not persisted — after a restart the clipboard
/// may hold anything, and claiming otherwise would be a lie on screen.
static CURRENT_CLIPBOARD_ID: AtomicI64 = AtomicI64::new(0);

/// Records which item is on the clipboard now, and tells the UI so it can mark
/// the card.
pub fn set_current_clipboard_id(app: &tauri::AppHandle, id: i64) {
    if CURRENT_CLIPBOARD_ID.swap(id, Ordering::SeqCst) != id {
        use tauri::Emitter;
        let _ = app.emit(model::events::CLIPBOARD_CURRENT, id);
    }
}

pub fn current_clipboard_id() -> Option<i64> {
    match CURRENT_CLIPBOARD_ID.load(Ordering::SeqCst) {
        0 => None,
        id => Some(id),
    }
}

/// Managed state, resolved by every command through `State<'_, AppState>`.
pub struct AppState {
    /// Keeps the non-blocking log writer alive. Dropping it shuts the writer
    /// down, and every later line is discarded — which reads exactly like the
    /// app going silent after startup.
    pub _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    pub store: Arc<Store>,
    pub settings: Arc<SettingsStore>,
    pub clipboard: Arc<ClipboardWatcher>,
    pub hotkeys: Arc<hotkey::HotkeyManager>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A second launch must surface the existing instance, never start a
        // second clipboard listener.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Err(e) = window::show_popup(app) {
                tracing::error!("second instance could not show the popup: {e}");
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // No baked arguments: the plugin writes them into the Run key once at
        // init, so a baked --silent could never be withdrawn when the user
        // turns silentStart off. The flag carried no information anyway — the
        // app reads settings.json at startup regardless.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Emitter;
            let handle = app.handle().clone();

            // Logging comes first. settings.json always lives under the default
            // root, so its directory is known without reading any settings, and
            // starting the logger afterwards threw away exactly the messages
            // worth having: the warning that settings.json was corrupt and was
            // replaced by defaults went nowhere, so a user whose settings were
            // silently reset had no way to find out.
            let settings_path = settings::default_store_root().join("settings.json");
            let log_guard = logging::init(&settings::default_store_root().join("logs"));

            let settings = Arc::new(SettingsStore::load(&settings_path)?);
            let resolved = settings.get();

            let store_root = resolved.store_root();
            if store_root != settings::default_store_root() {
                tracing::info!(
                    "store relocated to {}; logs stay under the default root",
                    store_root.display()
                );
            }

            tracing::info!("rebuffer starting, store at {}", store_root.display());

            // A configured store on a removable or network volume will one day
            // be gone at startup. Failing here would be unrecoverable for the
            // user: the tray icon never appears, so there is no way to open
            // settings and correct the path. Fall back to the default root and
            // say so loudly instead — a running app with an empty history can
            // be fixed, one that refuses to launch cannot.
            let (store, store_root) = match Store::open(&store_root) {
                Ok(s) => (Arc::new(s), store_root),
                Err(e) => {
                    let fallback = settings::default_store_root();
                    tracing::error!(
                        "store at {} could not be opened ({e}); falling back to {}",
                        store_root.display(),
                        fallback.display()
                    );
                    let s = Arc::new(Store::open(&fallback)?);
                    let _ = handle.emit(
                        model::events::STORE_UNAVAILABLE,
                        format!("{}", store_root.display()),
                    );
                    (s, fallback)
                }
            };

            // Granted only now: tauri.conf.json scopes the asset protocol to
            // the default store, a relocated one lives elsewhere, and doing
            // this before the open would have widened the scope to the path
            // that failed rather than the one actually in use — every
            // thumbnail would then silently fail to load.
            if let Err(e) = handle
                .asset_protocol_scope()
                .allow_directory(&store_root, true)
            {
                tracing::warn!(
                    "could not grant asset access to {}: {e}",
                    store_root.display()
                );
            }

            // The store deliberately does not read settings.json, so without
            // this it runs on its defaults — 30 days and no size cap — and a
            // user who set either would never see it take effect.
            store.set_retention_policy(model::RetentionPolicy {
                retention_days: resolved.storage.retention_days,
                max_store_bytes: resolved.storage.max_store_bytes.map(|b| b as i64),
            });

            let emit_handle = handle.clone();
            let clipboard = Arc::new(ClipboardWatcher::start(
                store.clone(),
                Box::new(move |item| {
                    use tauri::Emitter;
                    // A fresh capture IS what is on the clipboard right now.
                    set_current_clipboard_id(&emit_handle, item.id);
                    let _ = emit_handle.emit(model::events::ITEM_ADDED, item);
                }),
            )?);

            let hotkey_handle = handle.clone();
            let hotkeys = Arc::new(hotkey::HotkeyManager::new(Box::new(move || {
                // Never discard this: the hotkey firing but the window not
                // appearing is the single hardest failure to diagnose from the
                // outside, because both look like "the hotkey does not work".
                tracing::info!("hotkey fired");
                if let Err(e) = window::show_popup(&hotkey_handle) {
                    tracing::error!("hotkey fired but the popup did not show: {e}");
                }
            }))?);

            // HotkeyManager::new only builds the machinery; nothing is
            // registered until rebind runs, so without this the app starts with
            // no hotkey at all. A binding the user has made unusable must not
            // stop the app from starting — fall back to the default and log it,
            // because a tray app that refuses to launch is unrecoverable
            // without editing settings.json by hand.
            let chord = hotkey::Chord::parse(&resolved.hotkey.binding).unwrap_or_else(|e| {
                tracing::warn!(
                    "hotkey {:?} is not parseable ({e}), falling back to Alt+V",
                    resolved.hotkey.binding
                );
                hotkey::Chord::parse("Alt+V").expect("the default binding must always parse")
            });
            match hotkeys.rebind(&chord, resolved.hotkey.aggressive_mode) {
                Ok(()) => tracing::info!(
                    "hotkey {} registered (aggressive: {})",
                    chord.to_display(),
                    resolved.hotkey.aggressive_mode
                ),
                Err(e) => tracing::error!("could not register hotkey {}: {e}", chord.to_display()),
            }

            // Both windows are created hidden in tauri.conf.json; showing one
            // later costs a few milliseconds instead of the 300-600 ms a fresh
            // WebView2 would.
            for label in [window::POPUP_LABEL, window::SETTINGS_LABEL] {
                if let Some(w) = handle.get_webview_window(label) {
                    let _ = window::apply_backdrop(&w);
                }
            }

            tray::install(&handle)?;

            app.manage(AppState {
                _log_guard: log_guard,
                store,
                settings,
                clipboard,
                hotkeys,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::search_items,
            commands::get_item_blob_url,
            commands::get_extension_facets,
            commands::get_tab_counts,
            commands::get_storage_stats,
            commands::copy_to_clipboard,
            commands::paste_to_previous_window,
            commands::set_pinned,
            commands::rename_item,
            commands::delete_items,
            commands::add_files,
            commands::save_item_as,
            commands::open_item,
            commands::open_item_with,
            commands::show_in_folder,
            commands::begin_drag,
            commands::get_settings,
            commands::update_settings,
            commands::relocate_store,
            commands::export_data,
            commands::import_data,
            commands::set_capture_enabled,
            commands::run_cleanup_now,
            commands::clear_history,
            commands::get_current_clipboard_id,
            commands::get_clipboard_history_enabled,
            commands::set_clipboard_history_enabled,
            commands::hide_popup,
            commands::show_settings_window,
            commands::popup_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rebuffer");
}
