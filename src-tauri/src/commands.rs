//! The complete IPC surface. Mirrored by `src/lib/ipc.ts`.
//!
//! OWNER: the coordinator. Commands stay thin — they resolve state, delegate to
//! the owning module, and emit events. Put logic in the module, not here.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppResult;
use crate::model::{
    events, CleanupResult, Facet, Filter, ImportMode, ItemDto, Sort, StorageStats, TabCounts,
};
use crate::settings::Settings;
use crate::AppState;

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_items(
    state: State<'_, AppState>,
    filter: Filter,
    sort: Sort,
    offset: u32,
    limit: u32,
) -> AppResult<Vec<ItemDto>> {
    state.store.list(&filter, sort, offset, limit)
}

#[tauri::command]
pub fn search_items(
    state: State<'_, AppState>,
    query: String,
    filter: Filter,
    limit: u32,
) -> AppResult<Vec<ItemDto>> {
    state.store.search(&query, &filter, limit)
}

/// An `asset:` protocol URL the WebView can load directly, avoiding a base64
/// round trip for every thumbnail.
#[tauri::command]
pub fn get_item_blob_url(state: State<'_, AppState>, id: i64) -> AppResult<String> {
    let path = state.store.blob_path(id)?;
    // Windows serves Tauri's custom protocols over http://<scheme>.localhost,
    // not scheme://localhost — the CSP in tauri.conf.json lists both forms for
    // exactly this reason, and the asset:// spelling silently fails to load.
    Ok(format!(
        "http://asset.localhost/{}",
        urlencoding::encode(&path.to_string_lossy())
    ))
}

#[tauri::command]
pub fn get_extension_facets(state: State<'_, AppState>, filter: Filter) -> AppResult<Vec<Facet>> {
    state.store.ext_facets(&filter)
}

#[tauri::command]
pub fn get_tab_counts(state: State<'_, AppState>) -> AppResult<TabCounts> {
    state.store.tab_counts()
}

#[tauri::command]
pub fn get_storage_stats(state: State<'_, AppState>) -> AppResult<StorageStats> {
    state.store.stats()
}

// ---------------------------------------------------------------------------
// mutate
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn copy_to_clipboard(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    plain_text: bool,
) -> AppResult<()> {
    crate::clipboard::writer::write_items(&state.store, &ids, plain_text)?;
    // We wrote it, so we know what is on the clipboard — the listener skips our
    // own writes and would never report this one.
    if let [id] = ids[..] {
        crate::set_current_clipboard_id(&app, id);
    }
    Ok(())
}

/// The item currently on the clipboard, if we know. `None` after a restart, or
/// when something we never captured was copied.
#[tauri::command]
pub fn get_current_clipboard_id() -> Option<i64> {
    crate::current_clipboard_id()
}

#[tauri::command]
pub fn paste_to_previous_window(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    plain_text: bool,
) -> AppResult<()> {
    crate::clipboard::writer::write_items(&state.store, &ids, plain_text)?;
    if let [id] = ids[..] {
        crate::set_current_clipboard_id(&app, id);
    }
    crate::window::hide_popup(&app)?;
    if state.settings.get().behavior.auto_paste {
        crate::window::paste::send_paste()?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    pinned: bool,
) -> AppResult<()> {
    state.store.set_pinned(&ids, pinned)?;
    let _ = app.emit(events::ITEM_UPDATED, &ids);
    Ok(())
}

#[tauri::command]
pub fn rename_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    title: String,
) -> AppResult<()> {
    state.store.rename(id, &title)?;
    let _ = app.emit(events::ITEM_UPDATED, vec![id]);
    Ok(())
}

#[tauri::command]
pub fn delete_items(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> AppResult<()> {
    state.store.delete(&ids)?;
    let _ = app.emit(events::ITEMS_DELETED, &ids);
    Ok(())
}

#[tauri::command]
pub fn add_files(
    app: AppHandle,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> AppResult<Vec<ItemDto>> {
    let items = state.store.add_references(&paths)?;
    for item in &items {
        let _ = app.emit(events::ITEM_ADDED, item);
    }
    Ok(items)
}

#[tauri::command]
pub fn save_item_as(state: State<'_, AppState>, id: i64, target: String) -> AppResult<()> {
    let src = state.store.blob_path(id)?;
    std::fs::copy(src, target)?;
    Ok(())
}

#[tauri::command]
pub fn open_item(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::shell::open_item(&state.store, id)
}

#[tauri::command]
pub fn open_item_with(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::shell::open_item_with(&state.store, id)
}

#[tauri::command]
pub fn show_in_folder(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::shell::reveal(&state.store.blob_path(id)?)
}

/// Starts an OLE drag carrying `CF_HDROP`. Non-file items are materialized to
/// temp files with a sensible name first.
#[tauri::command]
pub fn begin_drag(state: State<'_, AppState>, ids: Vec<i64>) -> AppResult<()> {
    crate::shell::begin_drag(&state.store, &ids)
}

// ---------------------------------------------------------------------------
// settings & lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

/// Reads the live state of Windows clipboard history (Win+V) for this user.
/// OS state, not a stored preference — see `settings::clipboard_history_enabled`.
#[tauri::command]
pub fn get_clipboard_history_enabled() -> AppResult<bool> {
    crate::settings::clipboard_history_enabled()
}

/// Enables or disables Windows clipboard history (Win+V) for this user by
/// writing an explicit DWORD, never by deleting the value.
#[tauri::command]
pub fn set_clipboard_history_enabled(enabled: bool) -> AppResult<()> {
    crate::settings::set_clipboard_history_enabled(enabled)
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> AppResult<Settings> {
    let before = state.settings.get();

    // Validate the chord BEFORE anything is persisted. Saving first and
    // validating after leaves settings.json and the settings UI showing a
    // binding the runtime is not actually using, which is worse than a
    // rejected change: the user sees their new hotkey and it does nothing.
    let proposed_chord = match patch
        .get("hotkey")
        .and_then(|h| h.get("binding"))
        .and_then(|b| b.as_str())
    {
        Some(binding) if binding != before.hotkey.binding => {
            Some(crate::hotkey::Chord::parse(binding)?)
        }
        _ => None,
    };

    let next = state.settings.patch(patch)?;

    // Settings that own live OS state have to be pushed at whatever holds it;
    // saving the file changes nothing on its own.
    if proposed_chord.is_some() || next.hotkey.aggressive_mode != before.hotkey.aggressive_mode {
        let chord = match proposed_chord {
            Some(c) => c,
            None => crate::hotkey::Chord::parse(&next.hotkey.binding)?,
        };
        if let Err(e) = state.hotkeys.rebind(&chord, next.hotkey.aggressive_mode) {
            // The registration failed and the manager kept the old chord, so
            // roll the file back to match the runtime rather than letting the
            // two disagree. The rest of the patch stays applied: one refused
            // hotkey should not discard the user's other changes.
            let rollback = serde_json::json!({
                "hotkey": {
                    "binding": before.hotkey.binding,
                    "aggressiveMode": before.hotkey.aggressive_mode,
                }
            });
            let reverted = state.settings.patch(rollback)?;
            let _ = app.emit(events::SETTINGS_CHANGED, &reverted);
            return Err(e);
        }
    }

    if next.storage.retention_days != before.storage.retention_days
        || next.storage.max_store_bytes != before.storage.max_store_bytes
    {
        state
            .store
            .set_retention_policy(crate::model::RetentionPolicy {
                retention_days: next.storage.retention_days,
                max_store_bytes: next.storage.max_store_bytes.map(|b| b as i64),
            });
    }

    let _ = app.emit(events::SETTINGS_CHANGED, &next);
    Ok(next)
}

#[tauri::command]
pub fn relocate_store(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<()> {
    let target = std::path::PathBuf::from(&path);
    // Grant asset access before the move, not after: the frontend may request a
    // thumbnail the moment relocation finishes.
    if let Err(e) = app.asset_protocol_scope().allow_directory(&target, true) {
        tracing::warn!("could not grant asset access to {}: {e}", target.display());
    }
    crate::store::janitor::relocate(&app, &state.store, &target)
}

#[tauri::command]
pub fn export_data(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<()> {
    crate::store::janitor::export(&app, &state.store, std::path::Path::new(&path))
}

#[tauri::command]
pub fn import_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    mode: ImportMode,
) -> AppResult<()> {
    crate::store::janitor::import(&app, &state.store, std::path::Path::new(&path), mode)
}

#[tauri::command]
pub fn set_capture_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<()> {
    state.clipboard.set_enabled(enabled);
    crate::tray::set_capture_enabled(&app, enabled)?;
    Ok(())
}

#[tauri::command]
pub fn run_cleanup_now(
    state: State<'_, AppState>,
    older_than_days: Option<u32>,
) -> AppResult<CleanupResult> {
    state.store.run_cleanup(older_than_days)
}

/// The Data tab's Reset. Distinct from `run_cleanup_now`, which is the janitor
/// and deliberately spares pinned items and shelf references.
#[tauri::command]
pub fn clear_history(
    app: AppHandle,
    state: State<'_, AppState>,
    include_pinned: bool,
) -> AppResult<CleanupResult> {
    let result = state.store.clear_history(include_pinned)?;
    let _ = app.emit(events::ITEMS_DELETED, Vec::<i64>::new());
    Ok(result)
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) -> AppResult<()> {
    crate::window::hide_popup(&app)
}

#[tauri::command]
pub fn show_settings_window(app: AppHandle) -> AppResult<()> {
    crate::window::show_settings(&app)
}

/// Lets the popup restore its saved size after the WebView has laid out, which
/// avoids a visible resize flash on show.
#[tauri::command]
pub fn popup_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window(crate::window::POPUP_LABEL) {
        let _ = w.set_focus();
    }
}
