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
    CleanupResult, Facet, Filter, ItemDto, Kind, RetentionPolicy, Sort, StorageStats, SubKind,
    TabCounts,
};
use crate::preview;
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

/// One image waiting to be thumbnailed on the background thread. The row it
/// belongs to already exists and is already on screen, which is why the id
/// travels with it: the worker writes `thumb_path` back and tells the UI, and
/// until it does the row honestly reports having no thumbnail.
struct ThumbnailTask {
    pub item_id: i64,
    pub hash: String,
    pub bytes: Vec<u8>,
    pub root: PathBuf,
}

/// One captured link waiting to be looked up. Same shape and same reasoning as
/// `ThumbnailTask`: the row exists and is on screen, and gains a title and a
/// picture later, or never.
struct LinkTask {
    pub item_id: i64,
    pub hash: String,
    pub url: String,
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
    link_tx: Sender<LinkTask>,
    /// `privacy.linkPreviews`. Pushed in from settings, like the retention
    /// policy — the store does not read settings.json. Off means no link is
    /// ever looked up, so the check belongs before the queue, not inside the
    /// worker.
    link_previews: AtomicBool,
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
        let (link_tx, link_rx) = channel::<LinkTask>();

        let inner = Arc::new(StoreInner {
            root: RwLock::new(root.to_path_buf()),
            conn: Mutex::new(conn),
            retention_policy: RwLock::new(RetentionPolicy::default()),
            app_handle: Mutex::new(None),
            thumb_tx,
            link_tx,
            link_previews: AtomicBool::new(false),
            stop_janitor: AtomicBool::new(false),
        });

        let store = Store { inner };

        // Background worker for thumbnail generation. It owns the second half
        // of the capture: the row is inserted with no thumb_path, and this
        // thread fills it in and emits `item-updated` once the file is really
        // on disk. Announcing the path up front instead made the card request
        // a file that did not exist yet, take the <img> error path, and sit on
        // the placeholder until something happened to re-create the element —
        // which is exactly what a fresh screenshot looked like.
        //
        // A weak reference, like the janitor below, so StoreInner still drops.
        let weak_inner = Arc::downgrade(&store.inner);
        thread::Builder::new()
            .name("store-thumbnailer".into())
            .spawn(move || {
                use tauri::Emitter;
                while let Ok(task) = thumb_rx.recv() {
                    let rel = match write_thumbnail(&task.root, &task.hash, &task.bytes) {
                        Ok(Some(rel)) => rel,
                        // Undecodable image, or the write failed: the row keeps
                        // its NULL thumb_path and the card keeps the
                        // placeholder, which is the truth.
                        _ => continue,
                    };
                    let Some(inner) = weak_inner.upgrade() else {
                        return;
                    };
                    {
                        let conn = inner.conn.lock();
                        if let Err(e) = conn.execute(
                            "UPDATE items SET thumb_path = ?1 WHERE id = ?2",
                            rusqlite::params![rel, task.item_id],
                        ) {
                            tracing::warn!("thumbnail written but not recorded: {e}");
                            continue;
                        }
                    }
                    let app = inner.app_handle.lock().clone();
                    if let Some(app) = app {
                        let _ = app.emit(crate::model::events::ITEM_UPDATED, vec![task.item_id]);
                    }
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;

        // Link previews. Its own thread rather than a second kind of message
        // on the thumbnailer's channel: this one waits on a third party over
        // the network, and a slow lookup must never hold up the thumbnail of
        // the screenshot you just took.
        let weak_inner = Arc::downgrade(&store.inner);
        thread::Builder::new()
            .name("store-link-preview".into())
            .spawn(move || {
                use tauri::Emitter;
                while let Ok(task) = link_rx.recv() {
                    let found = match preview::fetch(&task.url) {
                        Ok(p) => p,
                        Err(e) => {
                            // Offline, blocked, deleted video, changed API:
                            // all of them leave the card exactly as it was.
                            tracing::debug!("link preview for {} failed: {e}", task.url);
                            continue;
                        }
                    };
                    // The picture is written before the row points at it, for
                    // the same reason capture thumbnails are.
                    let thumb = found
                        .thumbnail
                        .as_deref()
                        .and_then(|b| write_thumbnail(&task.root, &task.hash, b).ok().flatten());

                    let Some(inner) = weak_inner.upgrade() else {
                        return;
                    };
                    {
                        let conn = inner.conn.lock();
                        // COALESCE, so a title the user typed themselves and a
                        // thumbnail that is already there both win. The lookup
                        // fills a gap; it does not overwrite an answer.
                        if let Err(e) = conn.execute(
                            "UPDATE items
                             SET title = COALESCE(title, ?1),
                                 thumb_path = COALESCE(thumb_path, ?2)
                             WHERE id = ?3",
                            rusqlite::params![found.title, thumb, task.item_id],
                        ) {
                            tracing::warn!("link preview fetched but not recorded: {e}");
                            continue;
                        }
                    }
                    let app = inner.app_handle.lock().clone();
                    if let Some(app) = app {
                        let _ = app.emit(crate::model::events::ITEM_UPDATED, vec![task.item_id]);
                    }
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;

        // Run startup integrity sweep
        janitor::startup_sweep(&store)?;

        // Start hourly janitor background thread using a weak reference so StoreInner drops cleanly
        let weak_inner = Arc::downgrade(&store.inner);
        thread::Builder::new()
            .name("store-janitor".into())
            .spawn(move || loop {
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

    /// Turns link lookups on or off. Off is the default and stays the default
    /// until settings say otherwise, so a store built before the setting is
    /// pushed in cannot make a request.
    pub fn set_link_previews(&self, enabled: bool) {
        self.inner.link_previews.store(enabled, Ordering::SeqCst);
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

        // 4. Handle thumbnails for images OUTSIDE the SQLite lock. Only a
        //    thumbnail that already exists is recorded now; one that still has
        //    to be generated is queued after the insert, when the row has an id
        //    the worker can update and announce. The row is therefore correct
        //    at every instant: either it has a thumbnail whose file is there,
        //    or it has none.
        let mut pending_thumb: Option<Vec<u8>> = None;
        let thumb_path = if cap.kind == Kind::Image {
            let thumb_filename = format!("{}.webp", hash);
            let thumb_target = root.join("blobs").join("thumbs").join(&thumb_filename);
            if thumb_target.exists() {
                Some(thumb_filename)
            } else {
                pending_thumb = cap.primary.clone();
                None
            }
        } else {
            None
        };

        // 4b. A link worth looking up, if the user has asked for that. Decided
        //     here so the URL is captured before `cap` is taken apart, and
        //     queued after the insert for the same reason thumbnails are.
        let pending_link: Option<String> = if cap.sub_kind == Some(SubKind::Link) {
            cap.preview_text
                .as_deref()
                .filter(|u| preview::is_supported(u))
                .map(str::to_string)
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
            format_entries.push((
                fmt.format,
                fmt_blob_path,
                inline_data,
                fmt.bytes.len() as i64,
            ));
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
            let existing: Option<(i64, Option<String>)> = conn
                .query_row(
                    "SELECT id, title FROM items WHERE hash = ?1 AND is_reference = 0",
                    [&hash],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();

            if let Some((id, title)) = existing {
                conn.execute(
                    "UPDATE items SET created_at = ?1, copy_count = copy_count + 1 WHERE id = ?2",
                    rusqlite::params![now, id],
                )?;
                // A re-copy of an image whose thumbnail file went missing gets
                // it back, rather than showing the placeholder forever.
                self.queue_thumbnail(id, &hash, pending_thumb, &root);
                // Likewise a link copied again after previews were switched on:
                // re-copying it is the obvious way to ask for one, and it is
                // the only way an old row ever gets looked up. A row that
                // already has a title is left alone, so this cannot become a
                // request per copy.
                if title.is_none() {
                    self.queue_link_preview(id, &hash, pending_link.as_deref(), &root);
                }
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
                rusqlite::params![item_id, format, fmt_blob_path, inline_data, fmt_byte_size,],
            )?;
        }

        for (pos, file) in cap.files.into_iter().enumerate() {
            conn.execute(
                "INSERT INTO item_files (item_id, path, file_name, byte_size, position)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    item_id,
                    file.path,
                    file.file_name,
                    file.byte_size,
                    pos as i64
                ],
            )?;
        }

        self.queue_thumbnail(item_id, &hash, pending_thumb, &root);
        self.queue_link_preview(item_id, &hash, pending_link.as_deref(), &root);

        queries::get_item(&conn, &root, item_id)
    }

    /// Hands an image to the thumbnailer thread. A no-op when there is nothing
    /// to generate — either the item is not an image, or its thumbnail file
    /// already exists and the row already points at it.
    fn queue_thumbnail(&self, item_id: i64, hash: &str, bytes: Option<Vec<u8>>, root: &Path) {
        let Some(bytes) = bytes else { return };
        let _ = self.inner.thumb_tx.send(ThumbnailTask {
            item_id,
            hash: hash.to_string(),
            bytes,
            root: root.to_path_buf(),
        });
    }

    /// Hands a link to the preview thread. A no-op unless the user has turned
    /// previews on and the host is one `preview` is willing to contact — both
    /// checks live here, before anything is queued, so a disabled setting
    /// cannot leave a request sitting in a channel waiting to be sent.
    fn queue_link_preview(&self, item_id: i64, hash: &str, url: Option<&str>, root: &Path) {
        let Some(url) = url else { return };
        if !self.inner.link_previews.load(Ordering::SeqCst) {
            return;
        }
        let _ = self.inner.link_tx.send(LinkTask {
            item_id,
            hash: hash.to_string(),
            url: url.to_string(),
            root: root.to_path_buf(),
        });
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

    /// Looks up the links already in history that never got a preview, and
    /// returns how many were queued. Called when the setting is switched on,
    /// because that is the one moment the user has said yes to it: doing this
    /// at every startup would re-ask YouTube about the same dead videos
    /// forever, and doing it never would leave a history full of bare
    /// hostnames that no amount of waiting fixes.
    ///
    /// Bounded: a very long history is not worth a thousand requests in one
    /// burst, and anything past the cap is picked up by copying the link again.
    pub fn backfill_link_previews(&self) -> AppResult<usize> {
        const CAP: usize = 500;
        if !self.inner.link_previews.load(Ordering::SeqCst) {
            return Ok(0);
        }

        let root = self.root();
        let pending: Vec<(i64, String, String)> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT id, hash, preview_text FROM items
                 WHERE sub_kind = 'link' AND title IS NULL AND preview_text IS NOT NULL
                 ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.filter_map(|r| r.ok())
                .filter(|(_, _, url): &(i64, String, String)| preview::is_supported(url))
                .take(CAP)
                .collect()
        };

        for (id, hash, url) in &pending {
            self.queue_link_preview(*id, hash, Some(url), &root);
        }
        Ok(pending.len())
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
        assert_eq!(
            item1.preview_text.as_deref(),
            Some("Sample Text Content  \r\n")
        );

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

    /// A fresh screenshot used to show as an empty card: the row was announced
    /// with a thumb_path whose file the background thumbnailer had not written
    /// yet, so the card's <img> 404'd once and stayed on the placeholder. The
    /// row must never claim a thumbnail it does not have, and must gain one
    /// once the worker is done.
    #[test]
    fn an_image_gains_its_thumbnail_only_once_the_file_is_on_disk() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let img = image::RgbaImage::new(320, 200);
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let cap = Capture {
            kind: Kind::Image,
            sub_kind: None,
            primary: Some(png),
            formats: Vec::new(),
            files: Vec::new(),
            preview_text: None,
            ext: Some("PNG".into()),
            mime: Some("image/png".into()),
            width: Some(320),
            height: Some(200),
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        };

        let item = store.insert_capture(cap).unwrap();
        // Whatever the worker has managed by now, the URL and the file agree.
        let thumb_of = |it: &ItemDto| -> Option<PathBuf> {
            it.thumb_url.as_ref().map(|u| {
                let encoded = u.trim_start_matches("http://asset.localhost/");
                PathBuf::from(urlencoding::decode(encoded).unwrap().into_owned())
            })
        };
        if let Some(p) = thumb_of(&item) {
            assert!(p.exists(), "announced a thumbnail that is not on disk");
        }

        // And it does arrive: the worker writes the file, records the path and
        // the next read of the row carries it.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let fresh = store.get(item.id).unwrap();
            if let Some(p) = thumb_of(&fresh) {
                assert!(p.exists(), "thumb_path recorded before the file existed");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the thumbnailer never produced a thumbnail"
            );
            thread::sleep(Duration::from_millis(20));
        }
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
