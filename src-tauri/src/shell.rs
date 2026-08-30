//! Windows shell integration for the context menu and drag-out.
//!
//! OWNER: worker W4. `reveal` uses `SHOpenFolderAndSelectItems` rather than
//! `explorer /select` so it reuses an existing Explorer window; `open_with`
//! uses `SHOpenWithDialog`.

use std::path::Path;

use crate::error::AppResult;
use crate::store::Store;

pub fn open(_path: &Path) -> AppResult<()> {
    todo!("W4")
}

pub fn open_with(_path: &Path) -> AppResult<()> {
    todo!("W4")
}

pub fn reveal(_path: &Path) -> AppResult<()> {
    todo!("W4")
}

/// Phase 5. Materializes non-file items to temp files, then starts an OLE drag
/// carrying `CF_HDROP`.
pub fn begin_drag(_store: &Store, _ids: &[i64]) -> AppResult<()> {
    todo!("W4 — phase 5")
}
