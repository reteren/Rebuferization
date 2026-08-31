// OWNER: worker W6. The only frontend file that imports @tauri-apps/api.
// Mirrors src-tauri/src/commands.rs exactly: command names, argument names
// (camelCase, identical to the Rust parameter names), and event names from
// types.ts EVENTS. Change the Rust side, change this file, change both.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type { UnlistenFn } from '@tauri-apps/api/event'

import { EVENTS, type CleanupResult, type Facet, type Filter, type ImportMode, type ItemDto, type Settings, type SettingsPatch, type Sort, type StorageStats, type StorageWarning, type StoreProgress, type TabCounts } from './types'

// ---------------------------------------------------------------------------
// query
// ---------------------------------------------------------------------------

export function listItems(filter: Filter, sort: Sort, offset: number, limit: number): Promise<ItemDto[]> {
  return invoke<ItemDto[]>('list_items', { filter, sort, offset, limit })
}

export function searchItems(query: string, filter: Filter, limit: number): Promise<ItemDto[]> {
  return invoke<ItemDto[]>('search_items', { query, filter, limit })
}

/** An `asset:` protocol URL the WebView can load directly. */
export function getItemBlobUrl(id: number): Promise<string> {
  return invoke<string>('get_item_blob_url', { id })
}

export function getExtensionFacets(filter: Filter): Promise<Facet[]> {
  return invoke<Facet[]>('get_extension_facets', { filter })
}

export function getStorageStats(): Promise<StorageStats> {
  return invoke<StorageStats>('get_storage_stats')
}

export function getTabCounts(): Promise<TabCounts> {
  return invoke<TabCounts>('get_tab_counts')
}

/// The item sitting on the clipboard right now, or null when we cannot know —
/// after a restart, or when something we never captured was copied.
export function getCurrentClipboardId(): Promise<number | null> {
  return invoke<number | null>('get_current_clipboard_id')
}

// ---------------------------------------------------------------------------
// mutate
// ---------------------------------------------------------------------------

export function copyToClipboard(ids: number[], plainText: boolean): Promise<void> {
  return invoke<void>('copy_to_clipboard', { ids, plainText })
}

export function pasteToPreviousWindow(ids: number[], plainText: boolean): Promise<void> {
  return invoke<void>('paste_to_previous_window', { ids, plainText })
}

export function setPinned(ids: number[], pinned: boolean): Promise<void> {
  return invoke<void>('set_pinned', { ids, pinned })
}

export function renameItem(id: number, title: string): Promise<void> {
  return invoke<void>('rename_item', { id, title })
}

export function deleteItems(ids: number[]): Promise<void> {
  return invoke<void>('delete_items', { ids })
}

export function addFiles(paths: string[]): Promise<ItemDto[]> {
  return invoke<ItemDto[]>('add_files', { paths })
}

export function saveItemAs(id: number, target: string): Promise<void> {
  return invoke<void>('save_item_as', { id, target })
}

export function openItem(id: number): Promise<void> {
  return invoke<void>('open_item', { id })
}

export function openItemWith(id: number): Promise<void> {
  return invoke<void>('open_item_with', { id })
}

export function showInFolder(id: number): Promise<void> {
  return invoke<void>('show_in_folder', { id })
}

/** Starts an OLE drag carrying `CF_HDROP`; blocks until the drop completes. */
export function beginDrag(ids: number[]): Promise<void> {
  return invoke<void>('begin_drag', { ids })
}

// ---------------------------------------------------------------------------
// settings & lifecycle
// ---------------------------------------------------------------------------

export function getSettings(): Promise<Settings> {
  return invoke<Settings>('get_settings')
}

/** Live state of Windows clipboard history (Win+V) for this user. OS state,
 * not a stored preference — it lives in HKCU, and settings.json must never
 * shadow it. */
export function getClipboardHistoryEnabled(): Promise<boolean> {
  return invoke<boolean>('get_clipboard_history_enabled')
}

/** Enables or disables Windows clipboard history (Win+V) by writing an
 * explicit HKCU DWORD (1 or 0), never by deleting the value. */
export function setClipboardHistoryEnabled(enabled: boolean): Promise<void> {
  return invoke<void>('set_clipboard_history_enabled', { enabled })
}

export function updateSettings(patch: SettingsPatch): Promise<Settings> {
  return invoke<Settings>('update_settings', { patch })
}

export function relocateStore(path: string): Promise<void> {
  return invoke<void>('relocate_store', { path })
}

export function exportData(path: string): Promise<void> {
  return invoke<void>('export_data', { path })
}

export function importData(path: string, mode: ImportMode): Promise<void> {
  return invoke<void>('import_data', { path, mode })
}

export function setCaptureEnabled(enabled: boolean): Promise<void> {
  return invoke<void>('set_capture_enabled', { enabled })
}

export function runCleanupNow(olderThanDays: number | null): Promise<CleanupResult> {
  return invoke<CleanupResult>('run_cleanup_now', { olderThanDays })
}

/** The Data tab's Reset. Unlike run_cleanup_now (the janitor, which spares
 * pinned items and shelf references), clear_history takes them when asked. */
export function clearHistory(includePinned: boolean): Promise<CleanupResult> {
  return invoke<CleanupResult>('clear_history', { includePinned })
}

export function hidePopup(): Promise<void> {
  return invoke<void>('hide_popup')
}

export function showSettingsWindow(): Promise<void> {
  return invoke<void>('show_settings_window')
}

export function popupReady(): Promise<void> {
  return invoke<void>('popup_ready')
}

// ---------------------------------------------------------------------------
// events — payloads mirror model.rs `events` emissions
// ---------------------------------------------------------------------------

/// Fires with the id of the item that just became the clipboard's contents.
export function onClipboardCurrent(cb: (id: number) => void): Promise<UnlistenFn> {
  return listen<number>(EVENTS.clipboardCurrent, (e) => cb(e.payload))
}

export function onItemAdded(cb: (item: ItemDto) => void): Promise<UnlistenFn> {
  return listen<ItemDto>(EVENTS.itemAdded, (e) => cb(e.payload))
}

export function onItemsUpdated(cb: (ids: number[]) => void): Promise<UnlistenFn> {
  return listen<number[]>(EVENTS.itemUpdated, (e) => cb(e.payload))
}

export function onItemsDeleted(cb: (ids: number[]) => void): Promise<UnlistenFn> {
  return listen<number[]>(EVENTS.itemsDeleted, (e) => cb(e.payload))
}

export function onSettingsChanged(cb: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>(EVENTS.settingsChanged, (e) => cb(e.payload))
}

export function onStoreProgress(cb: (progress: StoreProgress) => void): Promise<UnlistenFn> {
  return listen<StoreProgress>(EVENTS.storeProgress, (e) => cb(e.payload))
}

export function onStorageWarning(cb: (warning: StorageWarning) => void): Promise<UnlistenFn> {
  return listen<StorageWarning>(EVENTS.storageWarning, (e) => cb(e.payload))
}