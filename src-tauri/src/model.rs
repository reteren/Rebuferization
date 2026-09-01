//! Shared domain types. These cross the IPC boundary, so every field name here
//! has a twin in `src/lib/types.ts` — change one, change both.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Image,
    Video,
    File,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::File => "file",
            Kind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Kind {
        match s {
            "text" => Kind::Text,
            "image" => Kind::Image,
            "video" => Kind::Video,
            "file" => Kind::File,
            _ => Kind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubKind {
    Plain,
    Rich,
    Code,
    Link,
    Color,
    Animated,
}

impl SubKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SubKind::Plain => "plain",
            SubKind::Rich => "rich",
            SubKind::Code => "code",
            SubKind::Link => "link",
            SubKind::Color => "color",
            SubKind::Animated => "animated",
        }
    }

    pub fn parse(s: &str) -> Option<SubKind> {
        Some(match s {
            "plain" => SubKind::Plain,
            "rich" => SubKind::Rich,
            "code" => SubKind::Code,
            "link" => SubKind::Link,
            "color" => SubKind::Color,
            "animated" => SubKind::Animated,
            _ => return None,
        })
    }
}

/// One row of `items`, shaped for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub id: i64,
    pub kind: Kind,
    pub sub_kind: Option<SubKind>,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub thumb_url: Option<String>,
    /// The ORIGINAL blob, populated only for `sub_kind = animated`. Thumbnails
    /// are a single decoded frame re-encoded as static WebP, so an animated GIF
    /// can never move when rendered from `thumb_url` — which is why
    /// `appearance.animateGifs` did nothing at all. The card picks between the
    /// two according to that setting.
    pub animated_url: Option<String>,
    pub ext: Option<String>,
    pub byte_size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub created_at: i64,
    pub pinned: bool,
    pub is_reference: bool,
    pub ref_path: Option<String>,
    pub source_app: Option<String>,
    pub copy_count: i64,
    /// A reference whose file no longer exists on disk.
    pub missing: bool,
    /// Only populated for multi-file `CF_HDROP` captures.
    pub file_names: Vec<String>,
}

/// Which tab / dropdown the grid is currently showing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// `None` = the All tab.
    pub kind: Option<Kind>,
    /// Uppercase, e.g. `"PNG"`. Narrows within `kind`.
    pub ext: Option<String>,
    /// The Pinned tab.
    pub pinned_only: bool,
    /// The Links tab, which is a sub_kind rather than a kind.
    pub sub_kind: Option<SubKind>,
    /// The Added Files tab: shelf items, stored by path and never copied.
    pub references_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
}

impl Sort {
    /// The `ORDER BY` fragment for this sort. Trusted, never user input.
    pub fn order_by(self) -> &'static str {
        match self {
            Sort::Newest => "created_at DESC",
            Sort::Oldest => "created_at ASC",
            Sort::NameAsc => "COALESCE(title, preview_text, ext) COLLATE NOCASE ASC",
            Sort::NameDesc => "COALESCE(title, preview_text, ext) COLLATE NOCASE DESC",
            Sort::SizeAsc => "byte_size ASC",
            Sort::SizeDesc => "byte_size DESC",
        }
    }

    /// Only the date sorts render group headers.
    pub fn is_grouped(self) -> bool {
        matches!(self, Sort::Newest | Sort::Oldest)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facet {
    pub ext: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindStat {
    pub kind: String,
    pub count: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub total_items: i64,
    pub total_bytes: i64,
    pub db_bytes: i64,
    pub by_kind: Vec<KindStat>,
    /// `None` when no cap is configured.
    pub cap_bytes: Option<i64>,
}

/// Item counts per popup tab. Derived counts would be wrong: `links` is a
/// sub_kind rather than a kind, `pinned` cuts across every kind, and extension
/// facets miss items with no extension — so the store counts them in SQL.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabCounts {
    pub all: i64,
    pub images: i64,
    pub text: i64,
    pub links: i64,
    pub files: i64,
    pub pinned: i64,
    /// Shelf items — added through + Add rather than captured.
    pub references: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub removed_items: i64,
    pub freed_bytes: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportMode {
    Merge,
    Replace,
}

/// What the janitor is allowed to delete. The store is deliberately ignorant of
/// `Settings` — depending on it would make the settings module and the store
/// mutually dependent — so the app pushes this snapshot in at startup and again
/// whenever the user changes it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    /// 1..=30.
    pub retention_days: u32,
    /// `None` = unlimited.
    pub max_store_bytes: Option<i64>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        RetentionPolicy {
            retention_days: 30,
            max_store_bytes: None,
        }
    }
}

/// Payload of the `storage-warning` event. Typed, because the frontend shows
/// real numbers in the toast rather than a prebaked sentence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageWarning {
    pub used_bytes: i64,
    pub cap_bytes: i64,
    /// Zero when this is the 90% warning fired before anything is deleted.
    pub removed_items: i64,
    pub freed_bytes: i64,
}

/// Payload of the `store-progress` event during a relocation / import / export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreProgress {
    pub phase: String,
    pub done: u64,
    pub total: u64,
}

/// Event names emitted to the frontend. Use these constants, never a literal.
pub mod events {
    pub const ITEM_ADDED: &str = "item-added";
    pub const ITEM_UPDATED: &str = "item-updated";
    pub const ITEMS_DELETED: &str = "items-deleted";
    pub const SETTINGS_CHANGED: &str = "settings-changed";
    pub const STORE_PROGRESS: &str = "store-progress";
    pub const STORAGE_WARNING: &str = "storage-warning";
    /// Carries the id of the item now on the clipboard, so exactly one card can
    /// be marked as live.
    pub const CLIPBOARD_CURRENT: &str = "clipboard-current";
    /// The configured store could not be opened and the app fell back to the
    /// default root. Carries the path that failed, so the UI can name it.
    pub const STORE_UNAVAILABLE: &str = "store-unavailable";
}
