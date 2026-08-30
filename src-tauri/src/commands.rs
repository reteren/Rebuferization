//! The complete IPC surface. Mirrored by `src/lib/ipc.ts`.
//!
//! OWNER: the coordinator. Commands stay thin — they resolve state, delegate to
//! the owning module, and emit events. Put logic in the module, not here.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppResult;
use crate::model::{
    events, CleanupResult, Facet, Filter, ImportMode, ItemDto, Sort, StorageStats,
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
    Ok(format!(
        "asset://localhost/{}",
        urlencoding::encode(&path.to_string_lossy())
    ))
}

#[tauri::command]
pub fn get_extension_facets(state: State<'_, AppState>, filter: Filter) -> AppResult<Vec<Facet>> {
    state.store.ext_facets(&filter)
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
    state: State<'_, AppState>,
    ids: Vec<i64>,
    plain_text: bool,
) -> AppResult<()> {
    crate::clipboard::writer::write_items(&state.store, &ids, plain_text)
}

#[tauri::command]
pub fn paste_to_previous_window(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    plain_text: bool,
) -> AppResult<()> {
    crate::clipboard::writer::write_items(&state.store, &ids, plain_text)?;
    crate::window::hide_popup(&app)?;
    if state.settings.get().behavior.auto_paste {
        crate::window::paste::send_paste()?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_pinned(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, pinned: bool) -> AppResult<()> {
    state.store.set_pinned(&ids, pinned)?;
    let _ = app.emit(events::ITEM_UPDATED, &ids);
    Ok(())
}

#[tauri::command]
pub fn rename_item(app: AppHandle, state: State<'_, AppState>, id: i64, title: String) -> AppResult<()> {
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
pub fn add_files(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> AppResult<Vec<ItemDto>> {
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
    crate::shell::open(&state.store.blob_path(id)?)
}

#[tauri::command]
pub fn open_item_with(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    crate::shell::open_with(&state.store.blob_path(id)?)
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

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> AppResult<Settings> {
    let next = state.settings.patch(patch)?;
    let _ = app.emit(events::SETTINGS_CHANGED, &next);
    Ok(next)
}

#[tauri::command]
pub fn relocate_store(app: AppHandle, state: State<'_, AppState>, path: String) -> AppResult<()> {
    crate::store::janitor::relocate(&app, &state.store, std::path::Path::new(&path))
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
pub fn set_capture_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> AppResult<()> {
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
