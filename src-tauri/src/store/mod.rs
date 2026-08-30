//! Persistence: SQLite metadata, content-addressed blobs, retention janitor.
//!
//! OWNER: worker W1. Everything under `store/` belongs to this module; nothing
//! else in the crate opens the database or touches `blobs/` directly.

pub mod blobs;
pub mod db;
pub mod janitor;
pub mod queries;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, MutexGuard, RwLock};
use rusqlite::Connection;

use tauri::AppHandle;

use crate::capture::Capture;
use crate::error::{AppError, AppResult};
use crate::model::{
    CleanupResult, Facet, Filter, ItemDto, Kind, RetentionPolicy, Sort, StorageStats, TabCounts,
};
use crate::store::blobs::{
    compute_files_hash, compute_hash, compute_text_hash, write_blob, write_thumbnail,
};

const DEFAULT_MAX_ITEM_BYTES: usize = 256 * 1024 * 1024; // 256 MB

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

struct ThumbnailTask {
    pub hash: String,
    pub bytes: Vec<u8>,
    pub root: PathBuf,
}

/// LOCK ORDERING:
/// Always acquire `root` (read lock) BEFORE acquiring `conn` (mutex lock).
/// Never acquire `root` while holding `conn`.
struct StoreInner {
    root: RwLock<PathBuf>,
    conn: Mutex<Connection>,
    retention_policy: RwLock<RetentionPolicy>,
    app_handle: Mutex<Option<AppHandle>>,
    thumb_tx: Sender<ThumbnailTask>,
    stop_janitor: AtomicBool,
}

/// The whole persistence layer. Cheap to clone (shares one pooled connection
/// behind a mutex), so it lives in Tauri managed state.
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

impl Store {
    /// Opens (creating if needed) the store at `root`, applies migrations, and
    /// runs the startup integrity sweep.
    pub fn open(root: &Path) -> AppResult<Store> {
        std::fs::create_dir_all(root)?;
        std::fs::create_dir_all(root.join("blobs"))?;
        std::fs::create_dir_all(root.join("blobs").join("thumbs"))?;

        let db_path = root.join("rebuffer.db");
        let conn = db::open_database(&db_path)?;

        let (thumb_tx, thumb_rx) = channel::<ThumbnailTask>();

        // Background worker thread for thumbnail generation
        thread::Builder::new()
            .name("store-thumbnailer".into())
            .spawn(move || {
                while let Ok(task) = thumb_rx.recv() {
                    let _ = write_thumbnail(&task.root, &task.hash, &task.bytes);
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;

        let inner = Arc::new(StoreInner {
            root: RwLock::new(root.to_path_buf()),
            conn: Mutex::new(conn),
            retention_policy: RwLock::new(RetentionPolicy::default()),
            app_handle: Mutex::new(None),
            thumb_tx,
            stop_janitor: AtomicBool::new(false),
        });

        let store = Store { inner };

        // Run startup integrity sweep
        janitor::startup_sweep(&store)?;

        // Start hourly janitor background thread using a weak reference so StoreInner drops cleanly
        let weak_inner = Arc::downgrade(&store.inner);
        thread::Builder::new()
            .name("store-janitor".into())
            .spawn(move || {
                loop {
                    for _ in 0..36000 {
                        thread::sleep(Duration::from_millis(100));
                        match weak_inner.upgrade() {
                            Some(inner) => {
                                if inner.stop_janitor.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                            None => return,
                        }
                    }
                    if let Some(inner) = weak_inner.upgrade() {
                        let store = Store { inner };
                        let policy = store.retention_policy();
                        let app = store.inner.app_handle.lock().clone();
                        let _ = janitor::run_cleanup_with_app(
                            app.as_ref(),
                            &store,
                            Some(policy.retention_days),
                            policy.max_store_bytes,
                        );
                    } else {
                        return;
                    }
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;

        Ok(store)
    }

    /// Root of the store folder — `%APPDATA%\Rebuffer` by default.
    pub fn root(&self) -> PathBuf {
        self.inner.root.read().clone()
    }

    /// Returns a lock guard to the SQLite connection.
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.inner.conn.lock()
    }

    /// Updates the retention policy for automatic and manual cleanups.
    pub fn set_retention_policy(&self, policy: RetentionPolicy) {
        *self.inner.retention_policy.write() = policy;
    }

    /// Returns a copy of the current retention policy.
    pub fn retention_policy(&self) -> RetentionPolicy {
        *self.inner.retention_policy.read()
    }

    /// Attaches the Tauri AppHandle for event emission.
    pub fn set_app_handle(&self, app: AppHandle) {
        *self.inner.app_handle.lock() = Some(app);
    }

    /// Updates the root directory and reopens the database connection (used during relocation).
    pub fn switch_root(&self, new_root: &Path) -> AppResult<()> {
        let mut root_guard = self.inner.root.write();
        let mut conn_guard = self.inner.conn.lock();

        let new_conn = db::open_database(&new_root.join("rebuffer.db"))?;
        *conn_guard = new_conn;
        *root_guard = new_root.to_path_buf();
        Ok(())
    }

    /// Persists a capture, or bumps the existing row when the hash already
    /// exists. Returns the row as the UI will see it.
    pub fn insert_capture(&self, cap: Capture) -> AppResult<ItemDto> {
        // 1. Enforce size limits BEFORE allocating or copying bytes
        if let Some(ref bytes) = cap.primary {
            if bytes.len() > DEFAULT_MAX_ITEM_BYTES {
                return Err(AppError::TooLarge(bytes.len() as u64));
            }
        }
        let total_format_bytes: usize = cap.formats.iter().map(|f| f.bytes.len()).sum();
        if total_format_bytes > DEFAULT_MAX_ITEM_BYTES {
            return Err(AppError::TooLarge(total_format_bytes as u64));
        }

        // 2. Compute BLAKE3 content hash
        let hash = if let Some(ref bytes) = cap.primary {
            if cap.kind == Kind::Text {
                let text = String::from_utf8_lossy(bytes);
                compute_text_hash(&text)
            } else {
                compute_hash(bytes)
            }
        } else if !cap.files.is_empty() {
            let paths: Vec<&str> = cap.files.iter().map(|f| f.path.as_str()).collect();
            compute_files_hash(&paths)
        } else if let Some(ref ref_path) = cap.ref_path {
            compute_hash(ref_path.as_bytes())
        } else if let Some(ref preview) = cap.preview_text {
            compute_text_hash(preview)
        } else {
            compute_hash(b"")
        };

        let root = self.root();

        // 3. Write primary blob to disk OUTSIDE the SQLite lock
        let blob_path = if let Some(ref bytes) = cap.primary {
            Some(write_blob(&root, &hash, bytes)?)
        } else {
            None
        };

        // 4. Handle thumbnails for images OUTSIDE the SQLite lock
        let thumb_path = if cap.kind == Kind::Image {
            let thumb_filename = format!("{}.webp", hash);
            let thumb_target = root.join("blobs").join("thumbs").join(&thumb_filename);
            if thumb_target.exists() {
                Some(thumb_filename)
            } else if let Some(ref bytes) = cap.primary {
                let _ = self.inner.thumb_tx.send(ThumbnailTask {
                    hash: hash.clone(),
                    bytes: bytes.clone(),
                    root: root.clone(),
                });
                Some(thumb_filename)
            } else {
                None
            }
        } else {
            None
        };

        // 5. Format blobs OUTSIDE the SQLite lock
        let mut format_entries = Vec::new();
        for fmt in cap.formats {
            let (inline_data, fmt_blob_path) = if fmt.bytes.len() <= 65536 {
                (Some(fmt.bytes.clone()), None)
            } else {
                let fmt_hash = compute_hash(&fmt.bytes);
                let rel = write_blob(&root, &fmt_hash, &fmt.bytes)?;
                (None, Some(rel))
            };
            format_entries.push((fmt.format, fmt_blob_path, inline_data, fmt.bytes.len() as i64));
        }

        let byte_size = if let Some(ref bytes) = cap.primary {
            bytes.len() as i64
        } else {
            cap.files.iter().filter_map(|f| f.byte_size).sum()
        };

        let now = current_time_ms();
        let is_ref_int = if cap.is_reference { 1 } else { 0 };

        // 6. Acquire SQLite connection ONLY for database operations
        let conn = self.conn();

        if !cap.is_reference {
            let existing: Option<i64> = conn
                .query_row(
                    "SELECT id FROM items WHERE hash = ?1 AND is_reference = 0",
                    [&hash],
                    |r| r.get(0),
                )
                .ok();

            if let Some(id) = existing {
                conn.execute(
                    "UPDATE items SET created_at = ?1, copy_count = copy_count + 1 WHERE id = ?2",
                    rusqlite::params![now, id],
                )?;
                return queries::get_item(&conn, &root, id);
            }
        }

        // Insert row into items table
        conn.execute(
            "INSERT INTO items (
                kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
                title, preview_text, ext, mime, byte_size, width, height, duration_ms,
                source_app, copy_count, created_at, first_seen_at, last_used_at, pinned
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 1, ?17, ?17, NULL, 0)",
            rusqlite::params![
                cap.kind.as_str(),
                cap.sub_kind.map(|s| s.as_str().to_string()),
                hash,
                blob_path,
                thumb_path,
                is_ref_int,
                cap.ref_path,
                Option::<String>::None,
                cap.preview_text,
                cap.ext,
                cap.mime,
                byte_size,
                cap.width,
                cap.height,
                cap.duration_ms,
                cap.source_app,
                now,
            ],
        )?;

        let item_id = conn.last_insert_rowid();

        for (format, fmt_blob_path, inline_data, fmt_byte_size) in format_entries {
            conn.execute(
                "INSERT INTO item_formats (item_id, format, blob_path, inline_data, byte_size)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    item_id,
                    format,
                    fmt_blob_path,
                    inline_data,
                    fmt_byte_size,
                ],
            )?;
        }

        for (pos, file) in cap.files.into_iter().enumerate() {
            conn.execute(
                "INSERT INTO item_files (item_id, path, file_name, byte_size, position)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![item_id, file.path, file.file_name, file.byte_size, pos as i64],
            )?;
        }

        queries::get_item(&conn, &root, item_id)
    }

    pub fn list(
        &self,
        filter: &Filter,
        sort: Sort,
        offset: u32,
        limit: u32,
    ) -> AppResult<Vec<ItemDto>> {
        let root = self.root();
        let conn = self.conn();
        queries::list_items(&conn, &root, filter, sort, offset, limit)
    }

    pub fn search(&self, query: &str, filter: &Filter, limit: u32) -> AppResult<Vec<ItemDto>> {
        let root = self.root();
        let conn = self.conn();
        queries::search_items(&conn, &root, query, filter, limit)
    }

    pub fn get(&self, id: i64) -> AppResult<ItemDto> {
        let root = self.root();
        let conn = self.conn();
        queries::get_item(&conn, &root, id)
    }

    /// Absolute path of an item's blob, or of the referenced file.
    pub fn blob_path(&self, id: i64) -> AppResult<PathBuf> {
        let root = self.root();
        let conn = self.conn();
        queries::get_blob_path(&conn, &root, id)
    }

    /// Every stored format for an item, in paste-restore order.
    pub fn formats(&self, id: i64) -> AppResult<Vec<(String, Vec<u8>)>> {
        let root = self.root();
        let conn = self.conn();
        queries::get_formats(&conn, &root, id)
    }

    pub fn set_pinned(&self, ids: &[i64], pinned: bool) -> AppResult<()> {
        let conn = self.conn();
        queries::set_pinned(&conn, ids, pinned)
    }

    pub fn rename(&self, id: i64, title: &str) -> AppResult<()> {
        let conn = self.conn();
        queries::rename_item(&conn, id, title)
    }

    pub fn delete(&self, ids: &[i64]) -> AppResult<()> {
        let root = self.root();
        let mut conn = self.conn();
        queries::delete_items(&mut conn, &root, ids)
    }

    pub fn add_references(&self, paths: &[String]) -> AppResult<Vec<ItemDto>> {
        let root = self.root();
        let conn = self.conn();
        queries::add_references(&conn, &root, paths)
    }

    pub fn touch_used(&self, id: i64) -> AppResult<()> {
        let conn = self.conn();
        queries::touch_used(&conn, id)
    }

    pub fn ext_facets(&self, filter: &Filter) -> AppResult<Vec<Facet>> {
        let conn = self.conn();
        queries::get_ext_facets(&conn, filter)
    }

    pub fn stats(&self) -> AppResult<StorageStats> {
        let root = self.root();
        let conn = self.conn();
        queries::get_storage_stats(&conn, &root)
    }

    /// One row of counts for the tab bar, in a single query.
    pub fn tab_counts(&self) -> AppResult<TabCounts> {
        let conn = self.conn();
        queries::get_tab_counts(&conn)
    }

    /// Deletes everything. `include_pinned` also removes pinned items and
    /// manual shelf references, which the janitor never touches — that is the
    /// difference between Clean now and Reset.
    pub fn clear_history(&self, include_pinned: bool) -> AppResult<CleanupResult> {
        let root = self.root();
        let conn = self.conn();
        queries::clear_history(&conn, &root, include_pinned)
    }

    /// Age sweep plus, if configured, the size cap. Safe to call repeatedly.
    pub fn run_cleanup(&self, older_than_days: Option<u32>) -> AppResult<CleanupResult> {
        let policy = self.retention_policy();
        let days = older_than_days.unwrap_or(policy.retention_days);
        let app = self.inner.app_handle.lock().clone();
        janitor::run_cleanup_with_app(app.as_ref(), self, Some(days), policy.max_store_bytes)
    }
}

impl Drop for StoreInner {
    fn drop(&mut self) {
        self.stop_janitor.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_store_insert_and_dedup() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let cap1 = Capture::text("Sample Text Content  \r\n");
        let item1 = store.insert_capture(cap1).unwrap();
        assert_eq!(item1.copy_count, 1);
        assert_eq!(item1.preview_text.as_deref(), Some("Sample Text Content  \r\n"));

        // Second capture with different whitespace/CRLF but same normalized text
        let cap2 = Capture::text("Sample Text Content\n");
        let item2 = store.insert_capture(cap2).unwrap();

        // Must bump existing row
        assert_eq!(item1.id, item2.id);
        assert_eq!(item2.copy_count, 2);

        // List should return only 1 item
        let list = store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_blob_refcounting_on_delete() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let data = b"identical image blob bytes";
        let cap = Capture {
            kind: Kind::Image,
            sub_kind: None,
            primary: Some(data.to_vec()),
            formats: Vec::new(),
            files: Vec::new(),
            preview_text: None,
            ext: Some("PNG".into()),
            mime: Some("image/png".into()),
            width: Some(100),
            height: Some(100),
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        };

        let item1 = store.insert_capture(cap).unwrap();
        let blob_path1 = store.blob_path(item1.id).unwrap();
        assert!(blob_path1.exists());

        // Delete item1
        store.delete(&[item1.id]).unwrap();
        // Since refcount reached 0, blob file must be deleted
        assert!(!blob_path1.exists());
    }
}
