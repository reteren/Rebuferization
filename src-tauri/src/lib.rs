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

use std::sync::Arc;

use tauri::Manager;

use crate::clipboard::ClipboardWatcher;
use crate::settings::SettingsStore;
use crate::store::Store;

/// Managed state, resolved by every command through `State<'_, AppState>`.
pub struct AppState {
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
            let _ = window::show_popup(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let settings_path = settings::default_store_root().join("settings.json");
            let settings = Arc::new(SettingsStore::load(&settings_path)?);
            let resolved = settings.get();

            let _log_guard = logging::init(&resolved.store_root().join("logs"));

            let store = Arc::new(Store::open(&resolved.store_root())?);

            let emit_handle = handle.clone();
            let clipboard = Arc::new(ClipboardWatcher::start(
                store.clone(),
                Box::new(move |item| {
                    use tauri::Emitter;
                    let _ = emit_handle.emit(model::events::ITEM_ADDED, item);
                }),
            )?);

            let hotkey_handle = handle.clone();
            let hotkeys = Arc::new(hotkey::HotkeyManager::new(Box::new(move || {
                let _ = window::show_popup(&hotkey_handle);
            }))?);

            // Both windows are created hidden in tauri.conf.json; showing one
            // later costs a few milliseconds instead of the 300-600 ms a fresh
            // WebView2 would.
            for label in [window::POPUP_LABEL, window::SETTINGS_LABEL] {
                if let Some(w) = handle.get_webview_window(label) {
                    let _ = window::apply_backdrop(&w);
                }
            }

            tray::install(&handle)?;

            app.manage(AppState { store, settings, clipboard, hotkeys });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::search_items,
            commands::get_item_blob_url,
            commands::get_extension_facets,
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
            commands::hide_popup,
            commands::show_settings_window,
            commands::popup_ready,
        ])
        .run(tauri::generate_context!())
        .expect("error while running rebuffer");
}
