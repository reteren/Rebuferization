//! Writing back to the clipboard.
//!
//! OWNER: worker W2. Bodies are yours; the signature is a contract used by
//! `commands.rs`.

use crate::error::AppResult;
use crate::store::Store;

/// Restores `ids` onto the clipboard. A single item restores every stored
/// format so Word-to-Word keeps its formatting; `plain_text` forces
/// `CF_UNICODETEXT` only. Multiple items join: text with newlines, files as one
/// `CF_HDROP`.
///
/// Records the resulting clipboard sequence number so the listener can
/// recognise this write as our own and skip it.
pub fn write_items(_store: &Store, _ids: &[i64], _plain_text: bool) -> AppResult<()> {
    todo!("W2")
}
