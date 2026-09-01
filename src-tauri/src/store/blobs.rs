//! Content-addressed blob store and thumbnail generator.
//!
//! Blobs are stored under `<root>/blobs/ab/cd/<hash>` with two-level fanout.
//! Thumbnails are stored under `<root>/blobs/thumbs/<hash>.webp`.
//!
//! OWNER: worker W1.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Computes BLAKE3 hash of raw bytes, returning a 64-character lowercase hex string.
pub fn compute_hash(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Normalizes text: converts CRLF/CR to LF and trims trailing whitespace.
pub fn normalize_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized.trim_end().to_string()
}

/// Computes BLAKE3 hash of normalized text.
pub fn compute_text_hash(text: &str) -> String {
    let normalized = normalize_text(text);
    compute_hash(normalized.as_bytes())
}

/// Computes BLAKE3 hash of a joined list of file paths.
pub fn compute_files_hash(paths: &[&str]) -> String {
    let joined = paths.join("\n");
    compute_hash(joined.as_bytes())
}

/// Relative path inside `blobs/` directory for a given hash: `ab/cd/<hash>`.
pub fn blob_rel_path(hash: &str) -> String {
    if hash.len() < 4 {
        format!("xx/yy/{}", hash)
    } else {
        format!("{}/{}/{}", &hash[0..2], &hash[2..4], hash)
    }
}

/// Relative path inside `blobs/thumbs/` for a given thumbnail: `<hash>.webp`.
pub fn thumb_rel_path(hash: &str) -> String {
    format!("{}.webp", hash)
}

/// Writes a blob atomically to disk under `<root>/blobs/ab/cd/<hash>`.
/// Returns the relative path `ab/cd/<hash>`.
pub fn write_blob(root: &Path, hash: &str, data: &[u8]) -> AppResult<String> {
    let rel = blob_rel_path(hash);
    let target_path = root.join("blobs").join(&rel);

    if target_path.exists() {
        return Ok(rel);
    }

    let parent = target_path
        .parent()
        .ok_or_else(|| AppError::Other("Invalid blob target directory".into()))?;
    std::fs::create_dir_all(parent)?;

    let temp_name = format!(
        "{}.tmp.{}_{}_{}",
        hash,
        std::process::id(),
        current_time_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = parent.join(&temp_name);

    // A `?` here used to leave the partial temp file behind for the rest of the
    // session — it was only cleaned on the rename-error path — and a full disk
    // is exactly when the write fails and exactly when the wasted bytes matter.
    // The startup sweep would eventually collect it, but not before a restart.
    {
        use std::io::Write;
        let write = (|| -> std::io::Result<()> {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(data)?;
            file.sync_all()
        })();
        if let Err(e) = write {
            let _ = std::fs::remove_file(&temp_path);
            return Err(AppError::Io(e));
        }
    }

    // Atomic rename into final location
    if let Err(e) = std::fs::rename(&temp_path, &target_path) {
        let _ = std::fs::remove_file(&temp_path);
        // If target was created concurrently by another thread, that's fine
        if !target_path.exists() {
            return Err(AppError::Io(e));
        }
    }

    Ok(rel)
}

/// Generates a WebP thumbnail (max 512px long edge, quality 80) and writes it
/// atomically to `<root>/blobs/thumbs/<hash>.webp`.
/// Returns `Ok(Some(filename))` on success, or `Ok(None)` if thumbnailing fails.
pub fn write_thumbnail(root: &Path, hash: &str, data: &[u8]) -> AppResult<Option<String>> {
    let thumbs_dir = root.join("blobs").join("thumbs");
    std::fs::create_dir_all(&thumbs_dir)?;

    let thumb_filename = thumb_rel_path(hash);
    let target_path = thumbs_dir.join(&thumb_filename);

    if target_path.exists() {
        return Ok(Some(thumb_filename));
    }

    let img = match image::load_from_memory(data) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Could not decode image for thumbnail: {}", e);
            return Ok(None);
        }
    };

    let (w, h) = (img.width(), img.height());
    let thumb_img = if w > 512 || h > 512 {
        img.resize(512, 512, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let rgba = thumb_img.to_rgba8();
    let encoder = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height());
    let webp_mem = encoder.encode(80.0);
    let webp_bytes = &*webp_mem;

    let temp_name = format!(
        "{}.tmp.{}_{}_{}",
        thumb_filename,
        std::process::id(),
        current_time_ms(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = thumbs_dir.join(&temp_name);

    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(webp_bytes)?;
        file.sync_all()?;
    }

    if let Err(e) = std::fs::rename(&temp_path, &target_path) {
        let _ = std::fs::remove_file(&temp_path);
        if !target_path.exists() {
            return Err(AppError::Io(e));
        }
    }

    Ok(Some(thumb_filename))
}

/// Checks derived refcount for `hash` across both `items` and `item_formats`,
/// deleting the on-disk blob and thumbnail only if the count reaches 0.
pub fn delete_blob_if_unreferenced(
    conn: &Connection,
    root: &Path,
    hash: &str,
    blob_path: Option<&str>,
    thumb_path: Option<&str>,
) -> AppResult<bool> {
    let count_items: i64 =
        conn.query_row("SELECT COUNT(*) FROM items WHERE hash = ?1", [hash], |r| {
            r.get(0)
        })?;

    let count_formats: i64 = if let Some(rel) = blob_path {
        conn.query_row(
            "SELECT COUNT(*) FROM item_formats WHERE blob_path = ?1",
            [rel],
            |r| r.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM item_formats WHERE blob_path LIKE ?1",
            [format!("%{}", hash)],
            |r| r.get(0),
        )?
    };

    if count_items + count_formats == 0 {
        if let Some(rel) = blob_path {
            let full_path = root.join("blobs").join(rel);
            if full_path.exists() {
                let _ = std::fs::remove_file(&full_path);
            }
        }
        if let Some(rel) = thumb_path {
            let full_path = root.join("blobs").join("thumbs").join(rel);
            if full_path.exists() {
                let _ = std::fs::remove_file(&full_path);
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Resolves an absolute path for a blob or file reference.
pub fn resolve_blob_or_ref(
    root: &Path,
    is_reference: bool,
    blob_path: Option<&str>,
    ref_path: Option<&str>,
) -> Option<PathBuf> {
    if is_reference {
        ref_path.map(PathBuf::from)
    } else {
        // blob_path is stored with forward slashes ("ab/cd/<hash>"), and
        // joining that verbatim yields a path with mixed separators. Ordinary
        // file APIs accept it, which is why everything else worked, but the
        // Windows shell does not: SHParseDisplayName answers E_INVALIDARG and
        // "Show in folder" fails. Split on the stored separator and join
        // component by component so the result is native throughout.
        blob_path.map(|p| {
            let mut out = root.join("blobs");
            for part in p.split('/').filter(|s| !s.is_empty()) {
                out.push(part);
            }
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The shell rejects a path with mixed separators even though every
    /// ordinary file API accepts it, so assert the shape rather than existence.
    #[test]
    fn resolve_blob_uses_native_separators_throughout() {
        let root = Path::new(r"C:\store");
        let p = resolve_blob_or_ref(root, false, Some("ab/cd/deadbeef"), None).unwrap();
        let s = p.to_string_lossy();
        assert!(
            !s.contains('/'),
            "a forward slash survived into the resolved path: {s}"
        );
        assert!(s.ends_with("deadbeef"));
        assert_eq!(p.components().count(), root.components().count() + 4);
    }

    #[test]
    fn test_text_normalization_and_hashing() {
        let text1 = "Hello World  \r\n";
        let text2 = "Hello World\n";
        assert_eq!(compute_text_hash(text1), compute_text_hash(text2));

        let norm = normalize_text("Line 1\r\nLine 2 \r\n");
        assert_eq!(norm, "Line 1\nLine 2");
    }

    #[test]
    fn test_blob_fanout_and_write() {
        let dir = tempdir().unwrap();
        let data = b"some test data for blob writing";
        let hash = compute_hash(data);

        let rel = write_blob(dir.path(), &hash, data).unwrap();
        assert_eq!(rel, format!("{}/{}/{}", &hash[0..2], &hash[2..4], hash));

        let full_path = dir.path().join("blobs").join(&rel);
        assert!(full_path.exists());
        assert_eq!(std::fs::read(&full_path).unwrap(), data);
    }

    #[test]
    fn test_thumbnail_generation() {
        let dir = tempdir().unwrap();
        // Create 600x400 test image (PNG)
        let img = image::RgbaImage::new(600, 400);
        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let hash = compute_hash(&png_bytes);
        let thumb_rel = write_thumbnail(dir.path(), &hash, &png_bytes).unwrap();
        assert!(thumb_rel.is_some());
        let thumb_name = thumb_rel.unwrap();
        assert_eq!(thumb_name, format!("{}.webp", hash));

        let thumb_full = dir.path().join("blobs").join("thumbs").join(&thumb_name);
        assert!(thumb_full.exists());

        // Verify it can be decoded and max dimension <= 512
        let decoded = image::load_from_memory(&std::fs::read(&thumb_full).unwrap()).unwrap();
        assert!(decoded.width() <= 512 && decoded.height() <= 512);
    }
}
