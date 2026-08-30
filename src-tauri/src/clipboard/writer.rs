//! Writing back to the clipboard.
//!
//! OWNER: worker W2. Bodies are yours; the signature is a contract used by
//! `commands.rs`.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND, POINT};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Shell::DROPFILES;

use crate::clipboard::decode::{FORMAT_IMAGE_PNG, FORMAT_PNG};
use crate::error::{AppError, AppResult};
use crate::model::Kind;
use crate::store::Store;

/// The sequence number resulting from our last clipboard write.
pub static LAST_WRITE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// Checks if a sequence number was recorded by our own writer.
pub fn is_our_sequence(seq: u32) -> bool {
    LAST_WRITE_SEQUENCE.load(Ordering::SeqCst) == seq
}

/// Records the sequence number of our last write.
pub fn record_sequence(seq: u32) {
    LAST_WRITE_SEQUENCE.store(seq, Ordering::SeqCst);
}

/// RAII Guard ensuring clipboard is always closed on any exit path.
pub struct ClipboardGuard(bool);

impl ClipboardGuard {
    /// Attempts to open clipboard with retry and backoff.
    pub fn open_with_retry(hwnd: Option<HWND>) -> AppResult<Self> {
        for attempt in 0..10 {
            unsafe {
                // Sound: OpenClipboard is passed an optional HWND (or None) to open the clipboard exclusively for current thread.
                if OpenClipboard(hwnd).is_ok() {
                    return Ok(ClipboardGuard(true));
                }
            }
            if attempt < 9 {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        Err(AppError::ClipboardBusy)
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                // Sound: CloseClipboard releases the clipboard lock previously acquired on this thread by OpenClipboard.
                let _ = CloseClipboard();
            }
        }
    }
}

/// Restores `ids` onto the clipboard. A single item restores every stored
/// format so Word-to-Word keeps its formatting; `plain_text` forces
/// `CF_UNICODETEXT` only. Multiple items join: text with newlines, files as one
/// `CF_HDROP`.
///
/// Records the resulting clipboard sequence number so the listener can
/// recognise this write as our own and skip it.
pub fn write_items(store: &Store, ids: &[i64], plain_text: bool) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }

    // Open clipboard with retry
    let _guard = ClipboardGuard::open_with_retry(None)?;

    unsafe {
        // Sound: Caller holds open clipboard lock via ClipboardGuard; EmptyClipboard clears contents and assigns ownership to current thread.
        EmptyClipboard()
            .map_err(|e| AppError::Other(format!("EmptyClipboard failed: {e}")))?;
    }

    if ids.len() == 1 {
        let id = ids[0];
        let item = store.get(id)?;

        if plain_text {
            let text = if let Some(ref preview) = item.preview_text {
                preview.clone()
            } else if let Ok(path) = store.blob_path(id) {
                if path.exists() {
                    std::fs::read_to_string(&path).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            set_clipboard_text(&text)?;
        } else {
            let formats = store.formats(id).unwrap_or_default();
            if !formats.is_empty() {
                for (fmt_name, bytes) in &formats {
                    let fmt_id = resolve_format_name(fmt_name);
                    if fmt_id != 0 {
                        let _ = set_clipboard_raw(fmt_id, bytes);
                    }
                }
                // Ensure CF_UNICODETEXT is also present if item has preview text
                if let Some(ref text) = item.preview_text {
                    let _ = set_clipboard_text(text);
                }
            } else {
                match item.kind {
                    Kind::Text => {
                        let text = if let Some(ref preview) = item.preview_text {
                            preview.clone()
                        } else if let Ok(path) = store.blob_path(id) {
                            if path.exists() {
                                std::fs::read_to_string(&path).unwrap_or_default()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        set_clipboard_text(&text)?;
                    }
                    Kind::Image => {
                        if let Ok(blob_path) = store.blob_path(id) {
                            if blob_path.exists() {
                                if let Ok(bytes) = std::fs::read(&blob_path) {
                                    // Set registered PNG format
                                    let _ = set_clipboard_raw(*FORMAT_PNG, &bytes);
                                    let _ = set_clipboard_raw(*FORMAT_IMAGE_PNG, &bytes);
                                    // Convert to CF_DIB for classic apps
                                    if let Ok(dib_bytes) = png_to_dib(&bytes) {
                                        let _ = set_clipboard_raw(8 /* CF_DIB */, &dib_bytes);
                                    }
                                }
                            }
                        }
                    }
                    Kind::File | Kind::Video => {
                        let paths = if !item.file_names.is_empty() {
                            // If multiple file paths or ref_path
                            if let Some(ref p) = item.ref_path {
                                vec![p.clone()]
                            } else if let Ok(p) = store.blob_path(id) {
                                vec![p.to_string_lossy().to_string()]
                            } else {
                                vec![]
                            }
                        } else if let Some(ref p) = item.ref_path {
                            vec![p.clone()]
                        } else if let Ok(p) = store.blob_path(id) {
                            vec![p.to_string_lossy().to_string()]
                        } else {
                            vec![]
                        };
                        if !paths.is_empty() {
                            set_clipboard_hdrop(&paths)?;
                            set_clipboard_text(&paths.join("\r\n"))?;
                        }
                    }
                    Kind::Other => {
                        if let Some(ref text) = item.preview_text {
                            set_clipboard_text(text)?;
                        }
                    }
                }
            }
        }
    } else {
        // Multiple items
        let mut items = Vec::with_capacity(ids.len());
        for &id in ids {
            items.push(store.get(id)?);
        }

        let all_files = items.iter().all(|it| matches!(it.kind, Kind::File | Kind::Video));

        if all_files && !plain_text {
            let mut paths = Vec::new();
            for it in &items {
                if let Some(ref p) = it.ref_path {
                    paths.push(p.clone());
                } else if let Ok(p) = store.blob_path(it.id) {
                    paths.push(p.to_string_lossy().to_string());
                }
            }
            if !paths.is_empty() {
                set_clipboard_hdrop(&paths)?;
                set_clipboard_text(&paths.join("\r\n"))?;
            }
        } else {
            // Join text with newlines
            let mut texts = Vec::new();
            for it in &items {
                if let Some(ref txt) = it.preview_text {
                    texts.push(txt.clone());
                } else if let Some(ref p) = it.ref_path {
                    texts.push(p.clone());
                } else if let Ok(p) = store.blob_path(it.id) {
                    texts.push(p.to_string_lossy().to_string());
                }
            }
            set_clipboard_text(&texts.join("\n"))?;
        }
    }

    // Explicitly drop clipboard guard to close before querying sequence number
    drop(_guard);

    unsafe {
        // Sound: GetClipboardSequenceNumber is a thread-safe Win32 query without preconditions or side effects.
        let seq = GetClipboardSequenceNumber();
        record_sequence(seq);
    }

    Ok(())
}

fn resolve_format_name(name: &str) -> u32 {
    match name {
        "CF_TEXT" => 1,
        "CF_BITMAP" => 2,
        "CF_METAFILEPICT" => 3,
        "CF_SYLK" => 4,
        "CF_DIF" => 5,
        "CF_TIFF" => 6,
        "CF_OEMTEXT" => 7,
        "CF_DIB" => 8,
        "CF_PALETTE" => 9,
        "CF_PENDATA" => 10,
        "CF_RIFF" => 11,
        "CF_WAVE" => 12,
        "CF_UNICODETEXT" => 13,
        "CF_ENHMETAFILE" => 14,
        "CF_HDROP" => 15,
        "CF_LOCALE" => 16,
        "CF_DIBV5" => 17,
        _ => {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                // Sound: wide slice is null-terminated UTF-16 and points to valid stack/heap memory for the duration of the call.
                RegisterClipboardFormatW(PCWSTR(wide.as_ptr()))
            }
        }
    }
}

/// Sets raw data on open clipboard for specified format ID.
pub fn set_clipboard_raw(format: u32, data: &[u8]) -> AppResult<()> {
    if data.is_empty() || format == 0 {
        return Ok(());
    }

    unsafe {
        // Sound: Allocates GMEM_MOVEABLE buffer of data.len() bytes, copies data into the locked memory region, unlocks it, and transfers ownership to the clipboard via SetClipboardData (or frees on failure).
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, data.len())
            .map_err(|e| AppError::Other(format!("GlobalAlloc raw failed: {e}")))?;
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other("GlobalLock raw failed".into()));
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
        let _ = GlobalUnlock(hglobal);

        let res = SetClipboardData(format, Some(HANDLE(hglobal.0)));
        if res.is_err() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other(format!("SetClipboardData {format} failed")));
        }
    }
    Ok(())
}

/// Writes UTF-16 text to `CF_UNICODETEXT` (13).
pub fn set_clipboard_text(text: &str) -> AppResult<()> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0); // Null terminator
    let byte_len = match utf16.len().checked_mul(2) {
        Some(len) => len,
        None => return Err(AppError::Other("Text byte length overflow".into())),
    };

    unsafe {
        // Sound: Allocates GMEM_MOVEABLE buffer for null-terminated UTF-16 text, locks and copies bytes, unlocks, and transfers ownership to clipboard via SetClipboardData with CF_UNICODETEXT.
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, byte_len)
            .map_err(|e| AppError::Other(format!("GlobalAlloc text failed: {e}")))?;
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other("GlobalLock text failed".into()));
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, ptr as *mut u8, byte_len);
        let _ = GlobalUnlock(hglobal);

        let res = SetClipboardData(13 /* CF_UNICODETEXT */, Some(HANDLE(hglobal.0)));
        if res.is_err() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other("SetClipboardData CF_UNICODETEXT failed".into()));
        }
    }
    Ok(())
}

/// Writes paths list to `CF_HDROP` (15).
pub fn set_clipboard_hdrop(paths: &[String]) -> AppResult<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut utf16_chars: Vec<u16> = Vec::new();
    for p in paths {
        utf16_chars.extend(p.encode_utf16());
        utf16_chars.push(0); // null separator between paths
    }
    utf16_chars.push(0); // double null terminator

    let dropfiles_size = std::mem::size_of::<DROPFILES>();
    let total_size = match utf16_chars.len().checked_mul(2).and_then(|bytes| bytes.checked_add(dropfiles_size)) {
        Some(size) => size,
        None => return Err(AppError::Other("DROPFILES total size overflow".into())),
    };

    unsafe {
        // Sound: Allocates GMEM_MOVEABLE buffer for DROPFILES header and double-null-terminated UTF-16 file paths, locks and initializes memory, unlocks, and transfers ownership to clipboard via SetClipboardData with CF_HDROP.
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, total_size)
            .map_err(|e| AppError::Other(format!("GlobalAlloc hdrop failed: {e}")))?;
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other("GlobalLock hdrop failed".into()));
        }

        let dropfiles = DROPFILES {
            pFiles: dropfiles_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1), // UTF-16
        };

        std::ptr::copy_nonoverlapping(
            &dropfiles as *const DROPFILES as *const u8,
            ptr as *mut u8,
            dropfiles_size,
        );

        std::ptr::copy_nonoverlapping(
            utf16_chars.as_ptr() as *const u8,
            (ptr as *mut u8).add(dropfiles_size),
            utf16_chars.len() * 2,
        );

        let _ = GlobalUnlock(hglobal);

        let res = SetClipboardData(15 /* CF_HDROP */, Some(HANDLE(hglobal.0)));
        if res.is_err() {
            let _ = GlobalFree(Some(hglobal));
            return Err(AppError::Other("SetClipboardData CF_HDROP failed".into()));
        }
    }
    Ok(())
}

/// Converts PNG bytes to a standard 32-bit `CF_DIB` structure (BITMAPINFOHEADER + BGRA pixels).
fn png_to_dib(png_bytes: &[u8]) -> AppResult<Vec<u8>> {
    let img = image::load_from_memory(png_bytes)?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();

    let header_size = 40usize;
    let pixel_bytes_len = match (width as usize).checked_mul(height as usize).and_then(|px| px.checked_mul(4)) {
        Some(len) => len,
        None => return Err(AppError::Other("Image dimensions overflow DIB size".into())),
    };
    let total_dib_len = match header_size.checked_add(pixel_bytes_len) {
        Some(len) => len,
        None => return Err(AppError::Other("DIB total length overflow".into())),
    };
    let mut dib = Vec::with_capacity(total_dib_len);

    // BITMAPINFOHEADER
    dib.extend_from_slice(&(header_size as u32).to_le_bytes()); // biSize
    dib.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight (bottom-up)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
    dib.extend_from_slice(&(pixel_bytes_len as u32).to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // Bottom-up pixel rows in BGRA format
    for y in (0..height).rev() {
        for x in 0..width {
            let px = rgba.get_pixel(x, y);
            dib.push(px[2]); // Blue
            dib.push(px[1]); // Green
            dib.push(px[0]); // Red
            dib.push(px[3]); // Alpha
        }
    }

    Ok(dib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_tracking() {
        record_sequence(42);
        assert!(is_our_sequence(42));
        assert!(!is_our_sequence(43));

        record_sequence(100);
        assert!(is_our_sequence(100));
        assert!(!is_our_sequence(42));
    }

    #[test]
    fn test_resolve_standard_format_names() {
        assert_eq!(resolve_format_name("CF_TEXT"), 1);
        assert_eq!(resolve_format_name("CF_DIB"), 8);
        assert_eq!(resolve_format_name("CF_UNICODETEXT"), 13);
        assert_eq!(resolve_format_name("CF_HDROP"), 15);
        assert_eq!(resolve_format_name("CF_DIBV5"), 17);
    }

    #[test]
    fn test_png_to_dib_conversion() {
        let mut img = image::RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255])); // Red
        img.put_pixel(1, 0, image::Rgba([0, 255, 0, 255])); // Green
        img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255])); // Blue
        img.put_pixel(1, 1, image::Rgba([255, 255, 0, 255])); // Yellow

        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        img.write_with_encoder(encoder).expect("PNG encode");

        let dib_bytes = png_to_dib(&png_bytes).expect("Convert PNG to DIB");
        assert_eq!(dib_bytes.len(), 40 + 2 * 2 * 4);

        // Verify DIB header
        let bi_size = u32::from_le_bytes(dib_bytes[0..4].try_into().unwrap_or_default());
        let bi_w = i32::from_le_bytes(dib_bytes[4..8].try_into().unwrap_or_default());
        let bi_h = i32::from_le_bytes(dib_bytes[8..12].try_into().unwrap_or_default());
        let bi_bpp = u16::from_le_bytes(dib_bytes[14..16].try_into().unwrap_or_default());

        assert_eq!(bi_size, 40);
        assert_eq!(bi_w, 2);
        assert_eq!(bi_h, 2);
        assert_eq!(bi_bpp, 32);
    }
}
