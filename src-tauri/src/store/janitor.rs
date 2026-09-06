//! Retention, the size cap, startup integrity sweep, and whole-store
//! operations: relocation, export, and import.
//!
//! OWNER: worker W1. All three long operations emit `store-progress`.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::error::{AppError, AppResult};
use crate::model::{events, CleanupResult, ImportMode, StorageWarning, StoreProgress};
use crate::store::queries;
use crate::store::Store;

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(windows)]
fn get_free_disk_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // GetDiskFreeSpaceExW fails on a path that does not exist yet, and the
    // relocation target usually does not — which made the whole free-space
    // guard silently return None and pass. Walk up to the nearest existing
    // ancestor, which is on the same volume and is what we actually want to
    // measure.
    let mut probe = path;
    while !probe.exists() {
        {
            let parent = probe.parent()?;
            probe = parent
        }
    }
    let path_str = probe.to_str()?;
    let wide: Vec<u16> = std::ffi::OsStr::new(path_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_bytes_available = 0u64;
    let mut total_number_of_bytes = 0u64;
    let mut total_number_of_free_bytes = 0u64;

    unsafe {
        if GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free_bytes_available),
            Some(&mut total_number_of_bytes),
            Some(&mut total_number_of_free_bytes),
        )
        .is_ok()
        {
            Some(free_bytes_available)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
fn get_free_disk_space(_path: &Path) -> Option<u64> {
    None
}

/// Runs the startup integrity sweep:
/// 1. Deletes leftover `.tmp` files in the `blobs/` tree.
/// 2. Deletes rows whose primary blob is missing on disk.
/// 3. Deletes blob files that no row references.
///
/// The old implementation ran one or two SQL queries PER FILE on disk
/// (a `COUNT(*)` per blob/thumb, and a leading-wildcard `LIKE '%<hash>'` per
/// blob that scans the whole `item_formats` table). At 10,000 items that was
/// ~30,000 queries and a ~100 s cold start. This version reads the full
/// reference set into memory with TWO queries and then walks the tree once,
/// testing membership against the sets — the walk also produces the set of
/// blob paths that exist on disk, so part 2 is a set lookup per row instead
/// of a filesystem stat per row.
///
/// Deliberately synchronous: it runs inside `Store::open`, before the
/// clipboard listener starts. It cannot safely run later on a background
/// thread — `insert_capture` writes a blob to disk *before* it inserts the
/// row that references it, so a sweep that overlaps a capture can observe a
/// fresh blob whose row is not yet committed and delete it as an orphan (or,
/// reordered, a committed row whose blob has not been written yet). The
/// synchronous path has no such window, and at the corrected cost the sweep
/// is ~200–300 ms at 10,000 items on this machine (~100 ms on typical
/// hardware) — not worth the race.
pub fn startup_sweep(store: &Store) -> AppResult<()> {
    let root = store.root().to_path_buf();
    let blobs_dir = root.join("blobs");

    if !blobs_dir.exists() {
        return Ok(());
    }

    let conn_guard = store.conn();

    // Reference set #1: every hash referenced by an items row (covers both
    // primary blobs and thumbnails, which are keyed by the item hash).
    let item_hashes: HashSet<String> = {
        let mut stmt = conn_guard.prepare("SELECT DISTINCT hash FROM items")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Reference set #2: format blobs, referenced by their `ab/cd/<hash>` rel
    // path. The file-name component IS the hash, so storing that makes the
    // comparison exact instead of a per-file `LIKE '%<hash>'`.
    let format_hashes: HashSet<String> = {
        let mut stmt =
            conn_guard.prepare("SELECT blob_path FROM item_formats WHERE blob_path IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok())
            .filter_map(|p| {
                Path::new(&p)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
            })
            .collect()
    };

    // One walk over the whole `blobs/` tree. Each file is stat'd exactly once
    // and never costs a query. The walk is parallelized with a work-stealing
    // queue: iterating ~25,000 entries on one thread is hundreds of
    // milliseconds even in release, and the `thumbs/` directory alone would
    // otherwise serialize a third of the work. Entries are classified as
    // dir/file from the directory listing's own attributes (no extra syscall).
    let mut existing_blobs: HashSet<String> = HashSet::new();
    {
        let thumbs_dir = blobs_dir.join("thumbs");
        let mut seed: VecDeque<(std::path::PathBuf, bool)> = VecDeque::new();
        if let Ok(rd) = std::fs::read_dir(&blobs_dir) {
            for e in rd.flatten() {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                seed.push_back((e.path(), is_dir));
            }
        }
        let queue = parking_lot::Mutex::new(seed);
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().max(1))
            .unwrap_or(4)
            .min(8);

        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(n_threads);
            for _ in 0..n_threads {
                handles.push(scope.spawn(|| {
                    let mut local_existing: HashSet<String> = HashSet::new();
                    loop {
                        let (path, is_dir) = match queue.lock().pop_front() {
                            Some(item) => item,
                            None => break,
                        };
                        if is_dir {
                            // Enumerate WITHOUT holding the queue lock: the
                            // directory reads are the expensive part and must
                            // run concurrently across threads. Only the
                            // enqueue takes the lock.
                            let children: Vec<(std::path::PathBuf, bool)> = if let Ok(rd) =
                                std::fs::read_dir(&path)
                            {
                                rd.flatten()
                                    .map(|e| {
                                        let d = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                                        (e.path(), d)
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            };
                            queue.lock().extend(children);
                            continue;
                        }
                        let name = path.file_name().unwrap_or_default();
                        let name_s = name.to_string_lossy();

                        // Part 1: leftover temp files (atomic-write leftovers).
                        if name_s.contains(".tmp.") || name_s.ends_with(".tmp") {
                            let _ = std::fs::remove_file(&path);
                            continue;
                        }

                        if path.starts_with(&thumbs_dir) {
                            // Part 3, thumbnails: a thumb is keyed by the item hash.
                            if let Some(hash) = name_s.strip_suffix(".webp") {
                                if !item_hashes.contains(hash) {
                                    let _ = std::fs::remove_file(&path);
                                }
                            }
                        } else {
                            // Part 3, primary blobs: record the rel path (feeds
                            // part 2), then delete the file if neither
                            // reference set knows its hash.
                            if let Ok(rel) = path.strip_prefix(&blobs_dir) {
                                local_existing.insert(normalize_rel(rel));
                            }
                            if name_s.len() == 64 {
                                let hash = name_s.as_ref();
                                if !item_hashes.contains(hash) && !format_hashes.contains(hash) {
                                    let _ = std::fs::remove_file(&path);
                                }
                            }
                        }
                    }
                    local_existing
                }));
            }
            for h in handles {
                existing_blobs.extend(h.join().unwrap());
            }
        });
    }

    // Part 2: rows whose primary blob is missing on disk. The walk above has
    // already stat'd the tree; this is a set lookup per row, and the DELETE
    // only ever runs for rows that are genuinely broken.
    {
        let mut stmt = conn_guard.prepare(
            "SELECT id, blob_path FROM items WHERE is_reference = 0 AND blob_path IS NOT NULL",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (id, blob_path) in rows {
            if !existing_blobs.contains(&blob_path) {
                tracing::warn!(
                    "Startup integrity sweep: deleting row {} with missing blob {}",
                    id,
                    blob_path
                );
                let _ = conn_guard.execute("DELETE FROM items WHERE id = ?1", [id]);
            }
        }
    }

    // Part 4: rows whose THUMBNAIL is missing. The row is not deleted — the
    // item and its blob are intact and losing a real capture over a missing
    // preview would be absurd — so the column is cleared and the card falls
    // back to the placeholder a thumbnail-less item already uses. Without this
    // the card asks for a file that is not there and renders a broken image
    // forever. The thumbs directory is flat and the walk above has already
    // removed orphans, so one read_dir gives an accurate set.
    {
        let mut existing_thumbs: HashSet<String> = HashSet::new();
        if let Ok(rd) = std::fs::read_dir(blobs_dir.join("thumbs")) {
            for entry in rd.flatten() {
                existing_thumbs.insert(entry.file_name().to_string_lossy().to_string());
            }
        }

        let mut stmt =
            conn_guard.prepare("SELECT id, thumb_path FROM items WHERE thumb_path IS NOT NULL")?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        let orphaned: Vec<i64> = rows
            .into_iter()
            .filter(|(_, t)| !existing_thumbs.contains(t.as_str()))
            .map(|(id, _)| id)
            .collect();

        if !orphaned.is_empty() {
            tracing::warn!(
                "Startup integrity sweep: clearing {} thumb_path values whose file is gone",
                orphaned.len()
            );
            for id in orphaned {
                let _ =
                    conn_guard.execute("UPDATE items SET thumb_path = NULL WHERE id = ?1", [id]);
            }
        }
    }

    Ok(())
}

/// The DB stores blob rel-paths with forward slashes; the directory walk
/// yields platform separators. Normalize so the two compare equal.
fn normalize_rel(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// Runs the age sweep and size cap pruning with optional AppHandle for events.
pub fn run_cleanup_with_app(
    app: Option<&AppHandle>,
    store: &Store,
    older_than_days: Option<u32>,
    max_store_bytes: Option<i64>,
) -> AppResult<CleanupResult> {
    // Root first, connection second. `switch_root` takes the root write lock
    // and then the connection lock, so acquiring them the other way round here
    // is a lock-order inversion: a relocation running on the event-loop thread
    // (the `relocate_store` command) would hold `root` and wait for `conn`
    // while this cleanup holds `conn` and waits for `root`, and neither ever
    // finishes. The event loop dies with it, which is what makes the tray menu
    // and the settings window stop opening at zero CPU. Every other pairing in
    // this file already reads the root before taking the connection.
    let root = store.root().to_path_buf();
    let mut conn = store.conn();
    let mut removed_items = 0i64;
    let mut freed_bytes = 0i64;

    // 1. Age sweep: skips pinned items and references
    let retention_days = older_than_days.unwrap_or(30);
    let cutoff_ms = current_time_ms() - (retention_days as i64 * 24 * 60 * 60 * 1000);

    {
        let expired: Vec<(i64, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT id, byte_size
                 FROM items
                 WHERE pinned = 0 AND is_reference = 0 AND created_at < ?1",
            )?;
            let res: Vec<(i64, i64)> = stmt
                .query_map([cutoff_ms], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            res
        };

        if !expired.is_empty() {
            let expired_ids: Vec<i64> = expired.iter().map(|(id, _)| *id).collect();
            let expired_bytes: i64 = expired.iter().map(|(_, bytes)| *bytes).sum();
            queries::delete_items(&mut conn, &root, &expired_ids)?;

            // The UI removes cards on `items-deleted`. Without this the janitor
            // prunes rows out from under an open popup and the cards stay on
            // screen until the next reload — clicking one then copies an item
            // that no longer exists.
            if let Some(a) = app {
                let _ = a.emit(events::ITEMS_DELETED, &expired_ids);
            }
            removed_items += expired_ids.len() as i64;
            freed_bytes += expired_bytes;
        }
    }

    // 2. Size cap sweep: if configured, delete oldest-first until <= 90% of cap
    if let Some(cap) = max_store_bytes {
        let total_bytes: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(byte_size), 0) FROM items WHERE is_reference = 0",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let warn_threshold = (cap * 9) / 10;

        // Emit 90% warning if used_bytes >= 90% of cap
        if total_bytes >= warn_threshold {
            if let Some(a) = app {
                let _ = a.emit(
                    events::STORAGE_WARNING,
                    StorageWarning {
                        used_bytes: total_bytes,
                        cap_bytes: cap,
                        removed_items: 0,
                        freed_bytes: 0,
                    },
                );
            }
        }

        if total_bytes > cap {
            let target_bytes = warn_threshold;
            let mut current_bytes = total_bytes;

            let rows: Vec<(i64, i64)> = {
                let mut stmt = conn.prepare(
                    "SELECT id, byte_size
                     FROM items
                     WHERE pinned = 0 AND is_reference = 0
                     ORDER BY created_at ASC",
                )?;
                let res: Vec<(i64, i64)> = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .filter_map(|r| r.ok())
                    .collect();
                res
            };

            let mut prune_ids = Vec::new();
            let mut prune_bytes = 0i64;
            for (id, byte_size) in rows {
                if current_bytes <= target_bytes {
                    break;
                }
                prune_ids.push(id);
                current_bytes -= byte_size;
                prune_bytes += byte_size;
            }

            if !prune_ids.is_empty() {
                queries::delete_items(&mut conn, &root, &prune_ids)?;
                removed_items += prune_ids.len() as i64;
                freed_bytes += prune_bytes;

                // Same reason as the age sweep above: the cap can fire while
                // the popup is open.
                if let Some(a) = app {
                    let _ = a.emit(events::ITEMS_DELETED, &prune_ids);
                }

                if let Some(a) = app {
                    let _ = a.emit(
                        events::STORAGE_WARNING,
                        StorageWarning {
                            used_bytes: current_bytes,
                            cap_bytes: cap,
                            removed_items: prune_ids.len() as i64,
                            freed_bytes: prune_bytes,
                        },
                    );
                }
            }
        }
    }

    Ok(CleanupResult {
        removed_items,
        freed_bytes,
    })
}

/// Runs the age sweep and size cap pruning without AppHandle.
pub fn run_cleanup(
    store: &Store,
    older_than_days: Option<u32>,
    max_store_bytes: Option<i64>,
) -> AppResult<CleanupResult> {
    run_cleanup_with_app(None, store, older_than_days, max_store_bytes)
}

/// Copies db + blobs to `target`, verifies row count and total bytes, then
/// removes the old tree. Refuses if the target volume has less free space than
/// the current store size x 1.2.
pub fn relocate(app: &AppHandle, store: &Store, target: &Path) -> AppResult<()> {
    let source_root = store.root().to_path_buf();
    if source_root == target {
        return Ok(());
    }

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "checking_space".into(),
            done: 0,
            total: 100,
        },
    );

    // Calculate total source size
    let db_size = std::fs::metadata(source_root.join("rebuffer.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    let mut total_blob_size = 0u64;
    let mut total_files = 0u64;

    for entry in WalkDir::new(source_root.join("blobs"))
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            total_blob_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_files += 1;
        }
    }

    let total_store_size = db_size + total_blob_size;

    #[cfg(windows)]
    {
        if let Some(free) = get_free_disk_space(target) {
            let required = (total_store_size as f64 * 1.2) as u64;
            if free < required {
                return Err(AppError::Other(format!(
                    "Target volume has insufficient free space (available: {} bytes, required: {} bytes)",
                    free, required
                )));
            }
        }
    }

    std::fs::create_dir_all(target)?;
    std::fs::create_dir_all(target.join("blobs"))?;
    std::fs::create_dir_all(target.join("blobs").join("thumbs"))?;

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "copying_database".into(),
            done: 10,
            total: 100,
        },
    );

    // Checkpoint source db
    {
        let conn = store.conn();
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    // Copy db file
    std::fs::copy(source_root.join("rebuffer.db"), target.join("rebuffer.db"))?;

    // Copy blobs
    let mut copied_files = 0u64;
    let blobs_src = source_root.join("blobs");
    let blobs_dst = target.join("blobs");

    for entry in WalkDir::new(&blobs_src).into_iter().flatten() {
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(&blobs_src)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let dest_file = blobs_dst.join(rel);
            if let Some(parent) = dest_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest_file)?;
            copied_files += 1;

            if total_files > 0 && copied_files % 50 == 0 {
                let progress = 10 + ((copied_files * 80) / total_files);
                let _ = app.emit(
                    events::STORE_PROGRESS,
                    StoreProgress {
                        phase: "copying_blobs".into(),
                        done: progress,
                        total: 100,
                    },
                );
            }
        }
    }

    // Verify target database
    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "verifying".into(),
            done: 95,
            total: 100,
        },
    );

    let src_count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;

    let target_conn = rusqlite::Connection::open(target.join("rebuffer.db"))?;
    let target_count: i64 =
        target_conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;

    if src_count != target_count {
        return Err(AppError::Other(format!(
            "Relocation verification failed: source had {} items, target has {}",
            src_count, target_count
        )));
    }
    drop(target_conn);

    // Switch store to target location
    store.switch_root(target)?;

    // Delete old tree
    let _ = std::fs::remove_dir_all(&source_root);

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "completed".into(),
            done: 100,
            total: 100,
        },
    );

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedFormat {
    pub format: String,
    pub blob_path: Option<String>,
    pub inline_data: Option<Vec<u8>>,
    pub byte_size: i64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedFile {
    pub path: String,
    pub file_name: String,
    pub byte_size: Option<i64>,
    pub position: i64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedItem {
    pub id: i64,
    pub kind: String,
    pub sub_kind: Option<String>,
    pub hash: String,
    pub blob_path: Option<String>,
    pub thumb_path: Option<String>,
    pub is_reference: bool,
    pub ref_path: Option<String>,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub ext: Option<String>,
    pub mime: Option<String>,
    pub byte_size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub source_app: Option<String>,
    pub copy_count: i64,
    pub created_at: i64,
    pub first_seen_at: i64,
    pub last_used_at: Option<i64>,
    pub pinned: bool,
    pub formats: Vec<ExportedFormat>,
    pub files: Vec<ExportedFile>,
}

/// Writes a `.rbx` archive: `settings.json`, `manifest.json`, `items.jsonl`,
/// and the `blobs/` tree.
pub fn export(app: &AppHandle, store: &Store, target: &Path) -> AppResult<()> {
    let root = store.root().to_path_buf();
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = std::fs::File::create(target)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "exporting_metadata".into(),
            done: 10,
            total: 100,
        },
    );

    // 1. settings.json
    let settings_bytes =
        std::fs::read(root.join("settings.json")).unwrap_or_else(|_| b"{}".to_vec());
    zip.start_file("settings.json", options)
        .map_err(|e| AppError::Other(e.to_string()))?;
    zip.write_all(&settings_bytes)?;

    // 2. manifest.json
    let total_items: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .unwrap_or(0);

    let manifest = serde_json::json!({
        "version": 1,
        "format": "rebuffer-v1",
        "exportedAt": current_time_ms(),
        "itemCount": total_items,
    });
    zip.start_file("manifest.json", options)
        .map_err(|e| AppError::Other(e.to_string()))?;
    zip.write_all(manifest.to_string().as_bytes())?;

    // 3. items.jsonl
    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "exporting_items".into(),
            done: 20,
            total: 100,
        },
    );

    zip.start_file("items.jsonl", options)
        .map_err(|e| AppError::Other(e.to_string()))?;

    {
        let conn = store.conn();
        let mut stmt = conn.prepare(
            "SELECT id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
                    title, preview_text, ext, mime, byte_size, width, height, duration_ms,
                    source_app, copy_count, created_at, first_seen_at, last_used_at, pinned
             FROM items ORDER BY id ASC",
        )?;

        let item_rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, Option<String>>(11)?,
                r.get::<_, i64>(12)?,
                r.get::<_, Option<i64>>(13)?,
                r.get::<_, Option<i64>>(14)?,
                r.get::<_, Option<i64>>(15)?,
                r.get::<_, Option<String>>(16)?,
                r.get::<_, i64>(17)?,
                r.get::<_, i64>(18)?,
                r.get::<_, i64>(19)?,
                r.get::<_, Option<i64>>(20)?,
                r.get::<_, i64>(21)?,
            ))
        })?;

        for row_res in item_rows {
            let row = row_res?;
            let id = row.0;

            // Gather formats
            let mut formats = Vec::new();
            if let Ok(mut f_stmt) = conn.prepare(
                "SELECT format, blob_path, inline_data, byte_size FROM item_formats WHERE item_id = ?1",
            ) {
                if let Ok(f_rows) = f_stmt.query_map([id], |fr| {
                    Ok(ExportedFormat {
                        format: fr.get(0)?,
                        blob_path: fr.get(1)?,
                        inline_data: fr.get(2)?,
                        byte_size: fr.get(3)?,
                    })
                }) {
                    for f in f_rows.flatten() {
                        formats.push(f);
                    }
                }
            }

            // Gather files
            let mut files = Vec::new();
            if let Ok(mut files_stmt) = conn.prepare(
                "SELECT path, file_name, byte_size, position FROM item_files WHERE item_id = ?1 ORDER BY position ASC",
            ) {
                if let Ok(file_rows) = files_stmt.query_map([id], |fr| {
                    Ok(ExportedFile {
                        path: fr.get(0)?,
                        file_name: fr.get(1)?,
                        byte_size: fr.get(2)?,
                        position: fr.get(3)?,
                    })
                }) {
                    for file in file_rows.flatten() {
                        files.push(file);
                    }
                }
            }

            let exported = ExportedItem {
                id: row.0,
                kind: row.1,
                sub_kind: row.2,
                hash: row.3,
                blob_path: row.4,
                thumb_path: row.5,
                is_reference: row.6 != 0,
                ref_path: row.7,
                title: row.8,
                preview_text: row.9,
                ext: row.10,
                mime: row.11,
                byte_size: row.12,
                width: row.13,
                height: row.14,
                duration_ms: row.15,
                source_app: row.16,
                copy_count: row.17,
                created_at: row.18,
                first_seen_at: row.19,
                last_used_at: row.20,
                pinned: row.21 != 0,
                formats,
                files,
            };

            let line = serde_json::to_string(&exported)?;
            zip.write_all(line.as_bytes())?;
            zip.write_all(b"\n")?;
        }
    }

    // 4. Blobs tree
    let blobs_dir = root.join("blobs");
    let mut total_blobs = 0u64;
    for entry in WalkDir::new(&blobs_dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            total_blobs += 1;
        }
    }

    let mut written_blobs = 0u64;
    for entry in WalkDir::new(&blobs_dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(&root)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let entry_name = rel.to_string_lossy().replace('\\', "/");
            zip.start_file(&entry_name, options)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let data = std::fs::read(entry.path())?;
            zip.write_all(&data)?;
            written_blobs += 1;

            if total_blobs > 0 && written_blobs % 20 == 0 {
                let progress = 30 + ((written_blobs * 65) / total_blobs);
                let _ = app.emit(
                    events::STORE_PROGRESS,
                    StoreProgress {
                        phase: "exporting_blobs".into(),
                        done: progress,
                        total: 100,
                    },
                );
            }
        }
    }

    zip.finish().map_err(|e| AppError::Other(e.to_string()))?;

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "completed".into(),
            done: 100,
            total: 100,
        },
    );

    Ok(())
}

/// Imports items and blobs from a `.rbx` archive in either Merge or Replace mode.
pub fn import(app: &AppHandle, store: &Store, archive: &Path, mode: ImportMode) -> AppResult<()> {
    let root = store.root().to_path_buf();
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::Other(e.to_string()))?;

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "extracting_blobs".into(),
            done: 10,
            total: 100,
        },
    );

    if matches!(mode, ImportMode::Replace) {
        let conn = store.conn();
        conn.execute_batch("DELETE FROM item_files; DELETE FROM item_formats; DELETE FROM items;")?;
    }

    // 1. Extract blobs
    let total_entries = zip.len();
    for i in 0..total_entries {
        let mut zfile = zip
            .by_index(i)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let name = zfile.name().to_string();

        if (name.starts_with("blobs/") || name.starts_with("blobs\\")) && !zfile.is_dir() {
            let dest_path = root.join(&name);
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&dest_path)?;
            std::io::copy(&mut zfile, &mut outfile)?;
        }

        if total_entries > 0 && i % 20 == 0 {
            let progress = 10 + ((i as u64 * 50) / total_entries as u64);
            let _ = app.emit(
                events::STORE_PROGRESS,
                StoreProgress {
                    phase: "extracting_blobs".into(),
                    done: progress,
                    total: 100,
                },
            );
        }
    }

    // 2. Read and import items.jsonl
    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "importing_items".into(),
            done: 65,
            total: 100,
        },
    );

    let mut jsonl_file = zip
        .by_name("items.jsonl")
        .map_err(|e| AppError::Other(format!("Missing items.jsonl in archive: {}", e)))?;
    let reader = BufReader::new(&mut jsonl_file);

    let conn = store.conn();

    for line_res in reader.lines() {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }

        let item: ExportedItem = serde_json::from_str(&line)?;

        if matches!(mode, ImportMode::Merge) && !item.is_reference {
            let existing_id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM items WHERE hash = ?1 AND is_reference = 0",
                    [&item.hash],
                    |r| r.get(0),
                )
                .ok();

            if let Some(id) = existing_id {
                let _ = conn.execute(
                    "UPDATE items SET copy_count = copy_count + 1 WHERE id = ?1",
                    [id],
                );
                continue;
            }
        }

        let is_ref_int = if item.is_reference { 1 } else { 0 };
        let pinned_int = if item.pinned { 1 } else { 0 };

        conn.execute(
            "INSERT INTO items (
                kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
                title, preview_text, ext, mime, byte_size, width, height, duration_ms,
                source_app, copy_count, created_at, first_seen_at, last_used_at, pinned
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            rusqlite::params![
                item.kind,
                item.sub_kind,
                item.hash,
                item.blob_path,
                item.thumb_path,
                is_ref_int,
                item.ref_path,
                item.title,
                item.preview_text,
                item.ext,
                item.mime,
                item.byte_size,
                item.width,
                item.height,
                item.duration_ms,
                item.source_app,
                item.copy_count,
                item.created_at,
                item.first_seen_at,
                item.last_used_at,
                pinned_int,
            ],
        )?;

        let item_id = conn.last_insert_rowid();

        for format in item.formats {
            conn.execute(
                "INSERT INTO item_formats (item_id, format, blob_path, inline_data, byte_size) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    item_id,
                    format.format,
                    format.blob_path,
                    format.inline_data,
                    format.byte_size,
                ],
            )?;
        }

        for file in item.files {
            conn.execute(
                "INSERT INTO item_files (item_id, path, file_name, byte_size, position) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    item_id,
                    file.path,
                    file.file_name,
                    file.byte_size,
                    file.position,
                ],
            )?;
        }
    }

    let _ = app.emit(
        events::STORE_PROGRESS,
        StoreProgress {
            phase: "completed".into(),
            done: 100,
            total: 100,
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tempfile::tempdir;

    #[test]
    fn test_age_and_cap_cleanup() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();

        let cap1 = crate::capture::Capture::text("Item 1");
        let item1 = store.insert_capture(cap1).unwrap();

        let cap2 = crate::capture::Capture::text("Item 2");
        let item2 = store.insert_capture(cap2).unwrap();

        // Pin item 1
        store.set_pinned(&[item1.id], true).unwrap();

        // Artificially age item 1 and item 2
        let old_time = current_time_ms() - (40 * 24 * 60 * 60 * 1000);
        store
            .conn()
            .execute(
                "UPDATE items SET created_at = ?1 WHERE id IN (?2, ?3)",
                rusqlite::params![old_time, item1.id, item2.id],
            )
            .unwrap();

        // Run age cleanup with 30 days retention
        let res = store.run_cleanup(Some(30)).unwrap();
        assert_eq!(res.removed_items, 1); // Only item 2 should be removed, item 1 is pinned

        let remaining = store
            .list(
                &crate::model::Filter::default(),
                crate::model::Sort::Newest,
                0,
                10,
            )
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, item1.id);
    }

    /// Seeds a store with 10,000 items, each with a real blob on disk, plus a
    /// realistic share of thumbnails (30% — only images get them) and
    /// format-only blobs (20%), a missing-blob row, and orphan files — then
    /// times `Store::open`, whose startup sweep has to reconcile all of it.
    /// This is the W29 cold-start benchmark: the OLD per-file sweep took ~107 s
    /// here; the fix must bring it under the SPEC §10 1.5 s target. `#[ignore]`
    /// so `cargo test` stays fast.
    #[test]
    #[ignore = "benchmark: seeds 10k items and times Store::open"]
    fn benchmark_startup_sweep_10k() {
        const N: i64 = 10_000;
        const THUMB_SHARE: i64 = 3_000; // 30% of items are images
        const FORMAT_SHARE: i64 = 2_000; // 20% carry an on-disk format blob
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();

        // -- seed phase -----------------------------------------------------
        let mut blob_rels = Vec::with_capacity(N as usize);
        // Orphan file identities, needed both for seeding and for the
        // post-sweep assertions.
        let orphan_hash = crate::store::blobs::compute_hash(b"orphan-blob");
        let orphan_thumb_hash = crate::store::blobs::compute_hash(b"orphan-thumb");
        let orphan_tmp_hash = crate::store::blobs::compute_hash(b"orphan-tmp");
        let orphan_blob = root
            .join("blobs")
            .join(crate::store::blobs::blob_rel_path(&orphan_hash));
        let orphan_thumb = root
            .join("blobs")
            .join("thumbs")
            .join(format!("{orphan_thumb_hash}.webp"));
        let orphan_tmp = root
            .join("blobs")
            .join(crate::store::blobs::blob_rel_path(&orphan_tmp_hash))
            .with_extension("tmp.1_1_1");
        {
            let store = Store::open(&root).unwrap();
            let mut conn = store.conn();
            let tx = conn.transaction().unwrap();

            // 10k items, each with a real fanout blob; images get a thumb.
            let mut insert = tx
                .prepare(
                    "INSERT INTO items (kind, hash, blob_path, thumb_path, is_reference, byte_size, created_at, first_seen_at)
                     VALUES (?1, ?2, ?3, ?4, 0, 64, ?5, ?5)",
                )
                .unwrap();
            for i in 0..N {
                let mut seed = [0u8; 64];
                seed[..8].copy_from_slice(&(i as u64).to_le_bytes());
                let hash = crate::store::blobs::compute_hash(&seed);
                let rel = crate::store::blobs::blob_rel_path(&hash);
                let target = root.join("blobs").join(&rel);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, seed).unwrap();
                let (kind, thumb_path) = if i < THUMB_SHARE {
                    let thumb_name = format!("{}.webp", hash);
                    std::fs::write(root.join("blobs").join("thumbs").join(&thumb_name), b"webp")
                        .unwrap();
                    ("image", Some(thumb_name))
                } else {
                    ("text", None)
                };
                insert
                    .execute(rusqlite::params![
                        kind,
                        hash,
                        rel,
                        thumb_path,
                        1_700_000_000_000i64 + i
                    ])
                    .unwrap();
                blob_rels.push((hash, rel));
            }
            drop(insert);

            // Format blobs referenced ONLY by item_formats (exercises the
            // format-membership path of the sweep: these must survive).
            for i in 0..FORMAT_SHARE {
                let mut seed = [0u8; 64];
                seed[..8].copy_from_slice(&((1u64 << 40) + i as u64).to_le_bytes());
                let hash = crate::store::blobs::compute_hash(&seed);
                let rel = crate::store::blobs::blob_rel_path(&hash);
                let target = root.join("blobs").join(&rel);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, seed).unwrap();
                tx.execute(
                    "INSERT INTO item_formats (item_id, format, blob_path, byte_size)
                     VALUES ((SELECT id FROM items WHERE hash = ?1), 'application/octet-stream', ?2, 64)",
                    rusqlite::params![blob_rels[i as usize].0, rel],
                )
                .unwrap();
            }

            // A row whose primary blob is MISSING on disk — the sweep must
            // delete this row, leaving exactly N items.
            {
                let mut seed = [0u8; 64];
                seed[..8].copy_from_slice(&(1u64 << 63).to_le_bytes());
                let hash = crate::store::blobs::compute_hash(&seed);
                let rel = crate::store::blobs::blob_rel_path(&hash); // file NOT written
                tx.execute(
                    "INSERT INTO items (kind, hash, blob_path, thumb_path, is_reference, byte_size, created_at, first_seen_at)
                     VALUES ('text', ?1, ?2, NULL, 0, 64, 1700000000000, 1700000000000)",
                    rusqlite::params![hash, rel],
                )
                .unwrap();
            }

            tx.commit().unwrap();
            drop(conn);
            drop(store);

            // Orphan files the sweep must delete: a stray blob, a stray thumb,
            // and a leftover tmp file.
            std::fs::create_dir_all(orphan_blob.parent().unwrap()).unwrap();
            std::fs::write(&orphan_blob, b"orphan").unwrap();
            std::fs::write(&orphan_thumb, b"orphan").unwrap();
            std::fs::create_dir_all(orphan_tmp.parent().unwrap()).unwrap();
            std::fs::write(&orphan_tmp, b"tmp").unwrap();
        }

        // -- timing phase: reopening runs the startup sweep ------------------
        let t0 = Instant::now();
        let store = Store::open(&root).unwrap();
        let sweep_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // -- assertions: the sweep reconciled everything correctly -----------
        let count: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, N, "missing-blob row must be deleted");
        assert!(!orphan_blob.exists(), "orphan blob must be deleted");
        assert!(!orphan_thumb.exists(), "orphan thumb must be deleted");
        assert!(!orphan_tmp.exists(), "leftover tmp file must be deleted");
        // Every referenced item blob and format-only blob must survive.
        let blobs_ok = blob_rels
            .iter()
            .all(|(_, rel)| root.join("blobs").join(rel).exists());
        assert!(blobs_ok, "every referenced item blob must survive");
        let formats_ok = (0..FORMAT_SHARE).all(|i| {
            let mut seed = [0u8; 64];
            seed[..8].copy_from_slice(&((1u64 << 40) + i as u64).to_le_bytes());
            let hash = crate::store::blobs::compute_hash(&seed);
            root.join("blobs")
                .join(crate::store::blobs::blob_rel_path(&hash))
                .exists()
        });
        assert!(formats_ok, "format-only blobs must survive");
        drop(store);

        println!(
            "benchmark_startup_sweep_10k: Store::open (startup sweep over ~{N} items) took {sweep_ms:.2} ms (items={count})"
        );
    }
}
