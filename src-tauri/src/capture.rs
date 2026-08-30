//! The handoff type between `clipboard` (producer) and `store` (consumer).
//!
//! `clipboard::listener` decodes a `WM_CLIPBOARDUPDATE` into exactly one
//! `Capture`; `store::Store::insert_capture` is the only thing that consumes
//! it. Neither side reaches across this boundary for anything else.

use crate::model::{Kind, SubKind};

/// One extra clipboard format kept so paste can restore the item faithfully.
#[derive(Debug, Clone)]
pub struct CapturedFormat {
    /// Windows format name, e.g. `"HTML Format"`, `"Rich Text Format"`.
    pub format: String,
    pub bytes: Vec<u8>,
}

/// One path from a `CF_HDROP` capture.
#[derive(Debug, Clone)]
pub struct CapturedFile {
    pub path: String,
    pub file_name: String,
    pub byte_size: Option<i64>,
}

/// What the clipboard held, decoded and ready to persist.
///
/// `hash` is deliberately absent: the store computes it, because the store owns
/// the deduplication rule (BLAKE3 over `primary` for blobs, over normalized
/// text for text, over the joined path list for file references).
#[derive(Debug, Clone)]
pub struct Capture {
    pub kind: Kind,
    pub sub_kind: Option<SubKind>,

    /// The canonical bytes for this item. `None` only for file captures, whose
    /// identity is `files` instead.
    pub primary: Option<Vec<u8>>,

    /// Additional formats to restore on paste. May be empty.
    pub formats: Vec<CapturedFormat>,

    /// Populated for `CF_HDROP`; empty otherwise.
    pub files: Vec<CapturedFile>,

    /// Searchable text and the text-card preview source. Present for text
    /// items, and for file items it is the joined file names.
    pub preview_text: Option<String>,

    /// Uppercase, no dot: `"PNG"`, `"TXT"`, `"MP4"`.
    pub ext: Option<String>,
    pub mime: Option<String>,

    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,

    /// Process name of the foreground window at capture time.
    pub source_app: Option<String>,

    /// A manual shelf add: store the path, never the bytes, and exempt it from
    /// the janitor.
    pub is_reference: bool,
    pub ref_path: Option<String>,
}

impl Capture {
    /// A minimal text capture, for tests and for the debug path.
    pub fn text(body: impl Into<String>) -> Capture {
        let body = body.into();
        Capture {
            kind: Kind::Text,
            sub_kind: Some(SubKind::Plain),
            primary: Some(body.as_bytes().to_vec()),
            formats: Vec::new(),
            files: Vec::new(),
            preview_text: Some(body),
            ext: Some("TXT".into()),
            mime: Some("text/plain".into()),
            width: None,
            height: None,
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        }
    }
}
