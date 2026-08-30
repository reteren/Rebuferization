//! Store read/write query operations.
//!
//! OWNER: worker W1.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::{
    CleanupResult, Facet, Filter, ItemDto, Kind, KindStat, Sort, StorageStats, SubKind, TabCounts,
};
use crate::store::blobs::{
    compute_hash, delete_blob_if_unreferenced, resolve_blob_or_ref, write_thumbnail,
};

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Raw representation of an `items` row before joining file names and resolving DTO paths.
pub struct RawItemRow {
    pub id: i64,
    pub kind_str: String,
    pub sub_kind_str: Option<String>,
    pub _hash: String,
    pub _blob_path: Option<String>,
    pub thumb_path: Option<String>,
    pub is_reference_int: i64,
    pub ref_path: Option<String>,
    pub title: Option<String>,
    pub preview_text: Option<String>,
    pub ext: Option<String>,
    pub _mime: Option<String>,
    pub byte_size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub source_app: Option<String>,
    pub copy_count: i64,
    pub created_at: i64,
    pub _first_seen_at: i64,
    pub _last_used_at: Option<i64>,
    pub pinned_int: i64,
}

impl RawItemRow {
    pub fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind_str: row.get(1)?,
            sub_kind_str: row.get(2)?,
            _hash: row.get(3)?,
            _blob_path: row.get(4)?,
            thumb_path: row.get(5)?,
            is_reference_int: row.get(6)?,
            ref_path: row.get(7)?,
            title: row.get(8)?,
            preview_text: row.get(9)?,
            ext: row.get(10)?,
            _mime: row.get(11)?,
            byte_size: row.get(12)?,
            width: row.get(13)?,
            height: row.get(14)?,
            duration_ms: row.get(15)?,
            source_app: row.get(16)?,
            copy_count: row.get(17)?,
            created_at: row.get(18)?,
            _first_seen_at: row.get(19)?,
            _last_used_at: row.get(20)?,
            pinned_int: row.get(21)?,
        })
    }
}

/// Converts a batch of `RawItemRow`s into `ItemDto`s, querying file names in a single batch.
pub fn map_rows_to_items(
    raw_rows: Vec<RawItemRow>,
    conn: &Connection,
    root: &Path,
) -> AppResult<Vec<ItemDto>> {
    if raw_rows.is_empty() {
        return Ok(Vec::new());
    }

    let file_ids: Vec<i64> = raw_rows
        .iter()
        .filter(|r| r.kind_str == "file" || r.is_reference_int != 0)
        .map(|r| r.id)
        .collect();

    let mut files_map: HashMap<i64, Vec<String>> = HashMap::new();
    if !file_ids.is_empty() {
        let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT item_id, file_name FROM item_files WHERE item_id IN ({}) ORDER BY item_id, position ASC",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = file_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let mut rows = stmt.query(params.as_slice())?;
        while let Some(row) = rows.next()? {
            let item_id: i64 = row.get(0)?;
            let file_name: String = row.get(1)?;
            files_map.entry(item_id).or_default().push(file_name);
        }
    }

    let items = raw_rows
        .into_iter()
        .map(|r| {
            let is_reference = r.is_reference_int != 0;
            let pinned = r.pinned_int != 0;
            let kind = Kind::parse(&r.kind_str);
            let sub_kind = r.sub_kind_str.as_deref().and_then(SubKind::parse);

            let thumb_url = r.thumb_path.map(|t| {
                let abs_thumb = root.join("blobs").join("thumbs").join(t);
                format!(
                    "asset://localhost/{}",
                    urlencoding::encode(&abs_thumb.to_string_lossy())
                )
            });

            let missing = if is_reference {
                if let Some(ref p) = r.ref_path {
                    !Path::new(p).exists()
                } else {
                    true
                }
            } else {
                false
            };

            let file_names = files_map.remove(&r.id).unwrap_or_default();

            ItemDto {
                id: r.id,
                kind,
                sub_kind,
                title: r.title,
                preview_text: r.preview_text,
                thumb_url,
                ext: r.ext,
                byte_size: r.byte_size,
                width: r.width,
                height: r.height,
                duration_ms: r.duration_ms,
                created_at: r.created_at,
                pinned,
                is_reference,
                ref_path: r.ref_path,
                source_app: r.source_app,
                copy_count: r.copy_count,
                missing,
                file_names,
            }
        })
        .collect();

    Ok(items)
}

const SELECT_ITEMS_COLUMNS: &str =
    "id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
     title, preview_text, ext, mime, byte_size, width, height, duration_ms,
     source_app, copy_count, created_at, first_seen_at, last_used_at, pinned";

/// Lists items with filtering, sorting, and pagination.
pub fn list_items(
    conn: &Connection,
    root: &Path,
    filter: &Filter,
    sort: Sort,
    offset: u32,
    limit: u32,
) -> AppResult<Vec<ItemDto>> {
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(kind) = filter.kind {
        conditions.push("kind = ?");
        params.push(Box::new(kind.as_str().to_string()));
    }
    if let Some(ref ext) = filter.ext {
        conditions.push("UPPER(ext) = UPPER(?)");
        params.push(Box::new(ext.clone()));
    }
    if filter.pinned_only {
        conditions.push("pinned = 1");
    }
    if let Some(sub_kind) = filter.sub_kind {
        conditions.push("sub_kind = ?");
        params.push(Box::new(sub_kind.as_str().to_string()));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT {} FROM items {} ORDER BY {} LIMIT ? OFFSET ?",
        SELECT_ITEMS_COLUMNS,
        where_sql,
        sort.order_by()
    );

    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let raw_rows: Vec<RawItemRow> = stmt
        .query_map(param_refs.as_slice(), RawItemRow::from_row)?
        .filter_map(|r| r.ok())
        .collect();

    map_rows_to_items(raw_rows, conn, root)
}

/// Searches items using FTS5 (with a LIKE fallback for short queries under 3 characters).
pub fn search_items(
    conn: &Connection,
    root: &Path,
    query: &str,
    filter: &Filter,
    limit: u32,
) -> AppResult<Vec<ItemDto>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return list_items(conn, root, filter, Sort::Newest, 0, limit);
    }

    if trimmed.chars().count() < 3 {
        return search_with_like(conn, root, trimmed, filter, limit);
    }

    // FTS5 query
    let clean_q = trimmed.replace('"', "\"\"");
    let fts_match = format!("\"{}\"*", clean_q);

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_match)];

    if let Some(kind) = filter.kind {
        conditions.push("i.kind = ?");
        params.push(Box::new(kind.as_str().to_string()));
    }
    if let Some(ref ext) = filter.ext {
        conditions.push("UPPER(i.ext) = UPPER(?)");
        params.push(Box::new(ext.clone()));
    }
    if filter.pinned_only {
        conditions.push("i.pinned = 1");
    }
    if let Some(sub_kind) = filter.sub_kind {
        conditions.push("i.sub_kind = ?");
        params.push(Box::new(sub_kind.as_str().to_string()));
    }

    let extra_where = if conditions.is_empty() {
        String::new()
    } else {
        format!("AND {}", conditions.join(" AND "))
    };

    let fts_columns =
        "i.id, i.kind, i.sub_kind, i.hash, i.blob_path, i.thumb_path, i.is_reference, i.ref_path,
         i.title, i.preview_text, i.ext, i.mime, i.byte_size, i.width, i.height, i.duration_ms,
         i.source_app, i.copy_count, i.created_at, i.first_seen_at, i.last_used_at, i.pinned";

    let sql = format!(
        "SELECT {} FROM items_fts f JOIN items i ON i.id = f.rowid WHERE items_fts MATCH ? {} ORDER BY rank LIMIT ?",
        fts_columns, extra_where
    );
    params.push(Box::new(limit as i64));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    match conn.prepare(&sql) {
        Ok(mut stmt) => match stmt.query_map(param_refs.as_slice(), RawItemRow::from_row) {
            Ok(rows) => {
                let raw_rows: Vec<RawItemRow> = rows.filter_map(|r| r.ok()).collect();
                map_rows_to_items(raw_rows, conn, root)
            }
            Err(_) => search_with_like(conn, root, trimmed, filter, limit),
        },
        Err(_) => search_with_like(conn, root, trimmed, filter, limit),
    }
}

fn search_with_like(
    conn: &Connection,
    root: &Path,
    query: &str,
    filter: &Filter,
    limit: u32,
) -> AppResult<Vec<ItemDto>> {
    let mut conditions = vec!["(preview_text LIKE ? OR title LIKE ?)".to_string()];
    let pattern = format!("%{}%", query);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(pattern.clone()),
        Box::new(pattern),
    ];

    if let Some(kind) = filter.kind {
        conditions.push("kind = ?".into());
        params.push(Box::new(kind.as_str().to_string()));
    }
    if let Some(ref ext) = filter.ext {
        conditions.push("UPPER(ext) = UPPER(?)".into());
        params.push(Box::new(ext.clone()));
    }
    if filter.pinned_only {
        conditions.push("pinned = 1".into());
    }
    if let Some(sub_kind) = filter.sub_kind {
        conditions.push("sub_kind = ?".into());
        params.push(Box::new(sub_kind.as_str().to_string()));
    }

    let where_sql = format!("WHERE {}", conditions.join(" AND "));
    let sql = format!(
        "SELECT {} FROM items {} ORDER BY created_at DESC LIMIT ?",
        SELECT_ITEMS_COLUMNS, where_sql
    );
    params.push(Box::new(limit as i64));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let raw_rows: Vec<RawItemRow> = stmt
        .query_map(param_refs.as_slice(), RawItemRow::from_row)?
        .filter_map(|r| r.ok())
        .collect();

    map_rows_to_items(raw_rows, conn, root)
}

/// Retrieves a single item by id.
pub fn get_item(conn: &Connection, root: &Path, id: i64) -> AppResult<ItemDto> {
    let sql = format!("SELECT {} FROM items WHERE id = ?1", SELECT_ITEMS_COLUMNS);
    let raw: RawItemRow = conn
        .query_row(&sql, [id], RawItemRow::from_row)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => AppError::NotFound(id),
            other => AppError::Db(other),
        })?;

    let mut items = map_rows_to_items(vec![raw], conn, root)?;
    items.pop().ok_or(AppError::NotFound(id))
}

/// Resolves the absolute path of an item's blob or referenced file.
pub fn get_blob_path(conn: &Connection, root: &Path, id: i64) -> AppResult<PathBuf> {
    let res: Option<(i64, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT is_reference, blob_path, ref_path FROM items WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    if let Some((is_ref, blob_path, ref_path)) = res {
        if let Some(p) = resolve_blob_or_ref(root, is_ref != 0, blob_path.as_deref(), ref_path.as_deref()) {
            return Ok(p);
        }
    }

    Err(AppError::NotFound(id))
}

/// Retrieves all stored clipboard formats for an item.
pub fn get_formats(conn: &Connection, root: &Path, id: i64) -> AppResult<Vec<(String, Vec<u8>)>> {
    let mut stmt = conn.prepare(
        "SELECT format, blob_path, inline_data FROM item_formats WHERE item_id = ?1 ORDER BY id ASC",
    )?;
    let mut rows = stmt.query([id])?;
    let mut result = Vec::new();

    while let Some(row) = rows.next()? {
        let format: String = row.get(0)?;
        let blob_path: Option<String> = row.get(1)?;
        let inline_data: Option<Vec<u8>> = row.get(2)?;

        if let Some(data) = inline_data {
            result.push((format, data));
        } else if let Some(rel) = blob_path {
            let full_path = root.join("blobs").join(rel);
            if full_path.exists() {
                let data = std::fs::read(full_path)?;
                result.push((format, data));
            }
        }
    }

    if result.is_empty() {
        let item_res: Option<(String, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT kind, blob_path, preview_text FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;

        if let Some((kind, blob_path, preview_text)) = item_res {
            if kind == "text" {
                if let Some(rel) = blob_path {
                    let full_path = root.join("blobs").join(rel);
                    if full_path.exists() {
                        let data = std::fs::read(full_path)?;
                        result.push(("CF_UNICODETEXT".into(), data));
                    }
                } else if let Some(txt) = preview_text {
                    result.push(("CF_UNICODETEXT".into(), txt.into_bytes()));
                }
            }
        }
    }

    Ok(result)
}

/// Sets the pinned status for a set of items.
pub fn set_pinned(conn: &Connection, ids: &[i64], pinned: bool) -> AppResult<()> {
    let pinned_val = if pinned { 1 } else { 0 };
    for &id in ids {
        conn.execute(
            "UPDATE items SET pinned = ?1 WHERE id = ?2",
            rusqlite::params![pinned_val, id],
        )?;
    }
    Ok(())
}

/// Renames an item.
pub fn rename_item(conn: &Connection, id: i64, title: &str) -> AppResult<()> {
    let affected = conn.execute(
        "UPDATE items SET title = ?1 WHERE id = ?2",
        rusqlite::params![title, id],
    )?;
    if affected == 0 {
        Err(AppError::NotFound(id))
    } else {
        Ok(())
    }
}

/// Deletes items and decrements derived blob refcounts.
pub fn delete_items(conn: &Connection, root: &Path, ids: &[i64]) -> AppResult<()> {
    for &id in ids {
        let row: Option<(String, Option<String>, Option<String>, i64)> = conn
            .query_row(
                "SELECT hash, blob_path, thumb_path, is_reference FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        if let Some((hash, blob_path, thumb_path, is_ref)) = row {
            conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
            if is_ref == 0 {
                delete_blob_if_unreferenced(
                    conn,
                    root,
                    &hash,
                    blob_path.as_deref(),
                    thumb_path.as_deref(),
                )?;
            }
        }
    }
    Ok(())
}

/// Adds file references to the store.
pub fn add_references(conn: &Connection, root: &Path, paths: &[String]) -> AppResult<Vec<ItemDto>> {
    let mut results = Vec::new();

    for path_str in paths {
        let path = Path::new(path_str);
        let exists = path.exists();
        let byte_size = if exists {
            std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
        } else {
            0
        };

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path_str)
            .to_string();

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_uppercase());

        let kind = match ext.as_deref() {
            Some("PNG" | "JPG" | "JPEG" | "GIF" | "WEBP" | "BMP" | "ICO" | "TIFF") => Kind::Image,
            Some("MP4" | "MKV" | "MOV" | "AVI" | "WEBM") => Kind::Video,
            Some("TXT" | "RS" | "JS" | "TS" | "PY" | "JSON" | "XML" | "MD" | "CSS" | "HTML" | "C" | "CPP" | "GO") => Kind::Text,
            _ => Kind::File,
        };

        let hash = compute_hash(path_str.as_bytes());

        // Check if reference already exists
        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM items WHERE ref_path = ?1 AND is_reference = 1",
                [path_str],
                |r| r.get(0),
            )
            .optional()?;

        let item_id = if let Some(id) = existing_id {
            let now = current_time_ms();
            conn.execute(
                "UPDATE items SET created_at = ?1, copy_count = copy_count + 1 WHERE id = ?2",
                rusqlite::params![now, id],
            )?;
            id
        } else {
            let thumb_path = if kind == Kind::Image && exists && byte_size <= 50 * 1024 * 1024 {
                if let Ok(bytes) = std::fs::read(path) {
                    write_thumbnail(root, &hash, &bytes)?
                } else {
                    None
                }
            } else {
                None
            };

            let now = current_time_ms();
            conn.execute(
                "INSERT INTO items (
                    kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
                    title, preview_text, ext, mime, byte_size, width, height, duration_ms,
                    source_app, copy_count, created_at, first_seen_at, last_used_at, pinned
                ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 1, ?16, ?16, NULL, 0)",
                rusqlite::params![
                    kind.as_str(),
                    Option::<String>::None,
                    hash,
                    Option::<String>::None,
                    thumb_path,
                    path_str,
                    file_name,
                    file_name,
                    ext,
                    Option::<String>::None,
                    byte_size,
                    Option::<i64>::None,
                    Option::<i64>::None,
                    Option::<i64>::None,
                    Option::<String>::None,
                    now,
                ],
            )?;

            let id = conn.last_insert_rowid();

            conn.execute(
                "INSERT INTO item_files (item_id, path, file_name, byte_size, position) VALUES (?1, ?2, ?3, ?4, 0)",
                rusqlite::params![id, path_str, file_name, byte_size],
            )?;

            id
        };

        results.push(get_item(conn, root, item_id)?);
    }

    Ok(results)
}

/// Updates the last_used_at timestamp on an item.
pub fn touch_used(conn: &Connection, id: i64) -> AppResult<()> {
    let now = current_time_ms();
    conn.execute(
        "UPDATE items SET last_used_at = ?1 WHERE id = ?2",
        rusqlite::params![now, id],
    )?;
    Ok(())
}

/// Computes extension facets.
pub fn get_ext_facets(conn: &Connection, filter: &Filter) -> AppResult<Vec<Facet>> {
    let mut conditions = vec!["ext IS NOT NULL".to_string(), "ext != ''".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(kind) = filter.kind {
        conditions.push("kind = ?".into());
        params.push(Box::new(kind.as_str().to_string()));
    }
    if filter.pinned_only {
        conditions.push("pinned = 1".into());
    }
    if let Some(sub_kind) = filter.sub_kind {
        conditions.push("sub_kind = ?".into());
        params.push(Box::new(sub_kind.as_str().to_string()));
    }

    let sql = format!(
        "SELECT UPPER(ext) as extension, COUNT(*) as count
         FROM items
         WHERE {}
         GROUP BY UPPER(ext)
         ORDER BY count DESC",
        conditions.join(" AND ")
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |r| {
        Ok(Facet {
            ext: r.get(0)?,
            count: r.get(1)?,
        })
    })?;

    let mut facets = Vec::new();
    for facet in rows {
        facets.push(facet?);
    }
    Ok(facets)
}

/// Gathers storage stats.
pub fn get_storage_stats(conn: &Connection, root: &Path) -> AppResult<StorageStats> {
    let total_items: i64 = conn
        .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .unwrap_or(0);

    let total_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(byte_size), 0) FROM items",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let db_path = root.join("rebuffer.db");
    let mut db_bytes = std::fs::metadata(&db_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    if let Ok(wal_m) = std::fs::metadata(root.join("rebuffer.db-wal")) {
        db_bytes += wal_m.len() as i64;
    }
    if let Ok(shm_m) = std::fs::metadata(root.join("rebuffer.db-shm")) {
        db_bytes += shm_m.len() as i64;
    }

    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*), COALESCE(SUM(byte_size), 0) FROM items GROUP BY kind",
    )?;
    let by_kind = stmt
        .query_map([], |r| {
            Ok(KindStat {
                kind: r.get(0)?,
                count: r.get(1)?,
                bytes: r.get(2)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(StorageStats {
        total_items,
        total_bytes,
        db_bytes,
        by_kind,
        cap_bytes: None,
    })
}

/// Retrieves item counts per popup tab in a single query.
pub fn get_tab_counts(conn: &Connection) -> AppResult<TabCounts> {
    let sql = "
        SELECT
            COUNT(*),
            COUNT(CASE WHEN kind = 'image' THEN 1 END),
            COUNT(CASE WHEN kind = 'text' THEN 1 END),
            COUNT(CASE WHEN sub_kind = 'link' THEN 1 END),
            COUNT(CASE WHEN kind = 'file' THEN 1 END),
            COUNT(CASE WHEN pinned = 1 THEN 1 END)
        FROM items;
    ";
    let counts = conn.query_row(sql, [], |r| {
        Ok(TabCounts {
            all: r.get(0)?,
            images: r.get(1)?,
            text: r.get(2)?,
            links: r.get(3)?,
            files: r.get(4)?,
            pinned: r.get(5)?,
        })
    })?;
    Ok(counts)
}

/// Clears history completely or unpinned only. Removes unreferenced blobs, clears FTS, and runs VACUUM.
pub fn clear_history(
    conn: &Connection,
    root: &Path,
    include_pinned: bool,
) -> AppResult<CleanupResult> {
    let where_clause = if include_pinned {
        ""
    } else {
        "WHERE pinned = 0 AND is_reference = 0"
    };

    let sql = format!(
        "SELECT id, hash, blob_path, thumb_path, byte_size, is_reference FROM items {}",
        where_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<(i64, String, Option<String>, Option<String>, i64, i64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut removed_items = 0i64;
    let mut freed_bytes = 0i64;

    for (id, hash, blob_path, thumb_path, byte_size, is_ref) in rows {
        conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
        if is_ref == 0 {
            delete_blob_if_unreferenced(
                conn,
                root,
                &hash,
                blob_path.as_deref(),
                thumb_path.as_deref(),
            )?;
        }
        removed_items += 1;
        freed_bytes += byte_size;
    }

    if include_pinned {
        let _ = conn.execute_batch(
            "DELETE FROM item_files;
             DELETE FROM item_formats;
             DELETE FROM items;
             DELETE FROM items_fts;",
        );
    }

    let _ = conn.execute_batch("VACUUM;");

    Ok(CleanupResult {
        removed_items,
        freed_bytes,
    })
}
