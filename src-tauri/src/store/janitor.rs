//! Retention, the size cap, and the whole-store operations that move bytes
//! around: relocation, export, import.
//!
//! OWNER: worker W1. All three long operations emit `store-progress`.

use std::path::Path;

use tauri::AppHandle;

use crate::error::AppResult;
use crate::model::ImportMode;
use crate::store::Store;

/// Copies db + blobs to `target`, verifies row count and total bytes, then
/// removes the old tree. Refuses if the target volume has less free space than
/// the current store size x 1.2.
pub fn relocate(_app: &AppHandle, _store: &Store, _target: &Path) -> AppResult<()> {
    todo!("W1")
}

/// Writes a `.rbx` archive: `settings.json`, `manifest.json`, `items.jsonl`,
/// and the `blobs/` tree.
pub fn export(_app: &AppHandle, _store: &Store, _target: &Path) -> AppResult<()> {
    todo!("W1")
}

pub fn import(
    _app: &AppHandle,
    _store: &Store,
    _archive: &Path,
    _mode: ImportMode,
) -> AppResult<()> {
    todo!("W1")
}
