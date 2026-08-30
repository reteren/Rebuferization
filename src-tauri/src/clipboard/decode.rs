//! Clipboard content decoding.
//!
//! Follows the priority order in SPEC §2.2:
//! 1. CF_HDROP (files / video references)
//! 2. CF_DIBV5 / CF_DIB / registered "PNG" format (images)
//! 3. "HTML Format" + CF_UNICODETEXT
//! 4. "Rich Text Format" + CF_UNICODETEXT
//! 5. CF_UNICODETEXT (plain text)
//!
//! OWNER: worker W2.

use std::path::Path;

use image::RgbaImage;
use once_cell::sync::Lazy;
use windows::core::w;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

use crate::capture::{Capture, CapturedFile, CapturedFormat};
use crate::clipboard::classify;
use crate::error::{AppError, AppResult};
use crate::model::{Kind, SubKind};

/// Registered format: "PNG"
pub static FORMAT_PNG: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and valid for program lifetime.
    RegisterClipboardFormatW(w!("PNG"))
});

/// Registered format: "image/png"
pub static FORMAT_IMAGE_PNG: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and valid for program lifetime.
    RegisterClipboardFormatW(w!("image/png"))
});

/// Registered format: "HTML Format"
pub static FORMAT_HTML: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and valid for program lifetime.
    RegisterClipboardFormatW(w!("HTML Format"))
});

/// Registered format: "Rich Text Format"
pub static FORMAT_RTF: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and valid for program lifetime.
    RegisterClipboardFormatW(w!("Rich Text Format"))
});

const CF_TEXT: u32 = 1;
const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;

/// Decodes the currently open clipboard contents into a `Capture`.
/// Assumes the clipboard is already open on this thread.
pub fn decode_clipboard(max_bytes: u64, source_app: Option<String>) -> AppResult<Option<Capture>> {
    // Priority 1: CF_HDROP
    if is_format_available(CF_HDROP) {
        if let Some(mut cap) = decode_hdrop(max_bytes)? {
            cap.source_app = source_app;
            return Ok(Some(cap));
        }
    }

    // Priority 2: Images (PNG / CF_DIBV5 / CF_DIB)
    if let Some(mut cap) = decode_image(max_bytes)? {
        cap.source_app = source_app;
        return Ok(Some(cap));
    }

    // Priority 3, 4, 5: Text formats (HTML, RTF, UnicodeText)
    if let Some(mut cap) = decode_text(max_bytes)? {
        cap.source_app = source_app;
        return Ok(Some(cap));
    }

    Ok(None)
}

fn is_format_available(format: u32) -> bool {
    if format == 0 {
        return false;
    }
    unsafe {
        // Sound: IsClipboardFormatAvailable performs a read-only query on clipboard format availability without side effects.
        IsClipboardFormatAvailable(format).is_ok()
    }
}

/// Decodes `CF_HDROP` files.
fn decode_hdrop(max_bytes: u64) -> AppResult<Option<Capture>> {
    unsafe {
        // Sound: Caller holds open clipboard lock; retrieves handle for CF_HDROP format owned by Windows.
        let handle = match GetClipboardData(CF_HDROP) {
            Ok(h) if !h.0.is_null() => h,
            _ => return Ok(None),
        };
        let hdrop = HDROP(handle.0);
        // Sound: Passing 0xFFFFFFFF queries the total count of dropped file paths.
        let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        if count == 0 {
            return Ok(None);
        }

        let mut files = Vec::with_capacity(count as usize);
        let mut total_bytes: u64 = 0;

        for i in 0..count {
            // Sound: Passing None for buffer queries required length (in chars) for file path at index i.
            let len = DragQueryFileW(hdrop, i, None);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len as usize + 1];
            // Sound: Buffer is sized len + 1 to receive the null-terminated UTF-16 path safely.
            let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
            if copied == 0 {
                continue;
            }
            let actual_copied = (copied as usize).min(buf.len());
            let path_str = String::from_utf16_lossy(&buf[..actual_copied]);
            let file_name = Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let byte_size = std::fs::metadata(&path_str)
                .map(|m| m.len() as i64)
                .ok();

            if let Some(size) = byte_size {
                total_bytes = total_bytes.saturating_add(size as u64);
                if total_bytes > max_bytes {
                    return Err(AppError::TooLarge(total_bytes));
                }
            }

            files.push(CapturedFile {
                path: path_str,
                file_name,
                byte_size,
            });
        }

        if files.is_empty() {
            return Ok(None);
        }

        let is_video = files.len() == 1 && classify::is_video_file(&files[0].path);
        let kind = if is_video { Kind::Video } else { Kind::File };

        let preview_text = files
            .iter()
            .map(|f| f.file_name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let ext = if files.len() == 1 {
            Path::new(&files[0].path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_uppercase())
        } else {
            None
        };

        Ok(Some(Capture {
            kind,
            sub_kind: None,
            primary: None,
            formats: Vec::new(),
            files,
            preview_text: Some(preview_text),
            ext,
            mime: None,
            width: None,
            height: None,
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        }))
    }
}

/// Decodes image formats (`PNG`, `CF_DIBV5`, `CF_DIB`).
fn decode_image(max_bytes: u64) -> AppResult<Option<Capture>> {
    // 1. Try registered PNG format
    for &png_fmt in &[*FORMAT_PNG, *FORMAT_IMAGE_PNG] {
        if is_format_available(png_fmt) {
            if let Some(png_bytes) = read_raw_clipboard_bytes(png_fmt, max_bytes) {
                if let Ok(img) = image::load_from_memory(&png_bytes) {
                    let w = img.width() as i64;
                    let h = img.height() as i64;
                    let sub_kind = if classify::is_animated_image(&png_bytes) {
                        Some(SubKind::Animated)
                    } else {
                        None
                    };
                    return Ok(Some(Capture {
                        kind: Kind::Image,
                        sub_kind,
                        primary: Some(png_bytes),
                        formats: Vec::new(),
                        files: Vec::new(),
                        preview_text: None,
                        ext: Some("PNG".into()),
                        mime: Some("image/png".into()),
                        width: Some(w),
                        height: Some(h),
                        duration_ms: None,
                        source_app: None,
                        is_reference: false,
                        ref_path: None,
                    }));
                }
            }
        }
    }

    // 2. Try CF_DIBV5 or CF_DIB
    let dib_format = if is_format_available(CF_DIBV5) {
        Some(CF_DIBV5)
    } else if is_format_available(CF_DIB) {
        Some(CF_DIB)
    } else {
        None
    };

    if let Some(fmt) = dib_format {
        unsafe {
            // Sound: Caller holds open clipboard lock; retrieves handle for DIB format owned by Windows.
            let handle = match GetClipboardData(fmt) {
                Ok(h) if !h.0.is_null() => h,
                _ => return Ok(None),
            };
            let hglobal = HGLOBAL(handle.0);
            // Sound: GlobalSize queries byte length of allocated global clipboard memory.
            let total_size = GlobalSize(hglobal);
            if total_size < 40 {
                return Ok(None);
            }
            // Sound: GlobalLock acquires pointer to global memory block valid until GlobalUnlock.
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                return Ok(None);
            }

            // Sound: ptr is valid for total_size bytes while locked by GlobalLock.
            let slice = std::slice::from_raw_parts(ptr as *const u8, total_size);

            if slice.len() < 40 {
                // Sound: GlobalUnlock releases lock on hglobal before early return.
                let _ = GlobalUnlock(hglobal);
                return Ok(None);
            }

            // Verify size limit from header BEFORE decoding full buffer with overflow protection
            let bi_width_raw = match read_i32_le(&slice[4..8]) {
                Some(w) => w,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };
            let bi_height_raw = match read_i32_le(&slice[8..12]) {
                Some(h) => h,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };
            let bi_bit_count = match read_u16_le(&slice[14..16]) {
                Some(b) => b as u64,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };

            let bi_width = match bi_width_raw.checked_abs() {
                Some(w) if w > 0 => w as u64,
                _ => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };
            let bi_height = match bi_height_raw.checked_abs() {
                Some(h) if h > 0 => h as u64,
                _ => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };

            if bi_bit_count == 0 || !matches!(bi_bit_count, 1 | 4 | 8 | 16 | 24 | 32) {
                let _ = GlobalUnlock(hglobal);
                return Ok(None);
            }

            let bits_per_row = match bi_width.checked_mul(bi_bit_count) {
                Some(bits) => bits,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };

            let row_stride = match bits_per_row.checked_add(31).map(|b| (b / 32) * 4) {
                Some(stride) => stride,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };

            let estimated_pixel_bytes = match row_stride.checked_mul(bi_height) {
                Some(p) => p,
                None => {
                    let _ = GlobalUnlock(hglobal);
                    return Ok(None);
                }
            };

            if estimated_pixel_bytes > max_bytes || (total_size as u64) > max_bytes {
                // Sound: GlobalUnlock releases lock on hglobal before returning error.
                let _ = GlobalUnlock(hglobal);
                return Err(AppError::TooLarge(estimated_pixel_bytes.max(total_size as u64)));
            }

            let dib_bytes = slice.to_vec();
            // Sound: GlobalUnlock releases lock on hglobal after copying buffer into owned Vec.
            let _ = GlobalUnlock(hglobal);

            let rgba = match decode_dib_to_rgba(&dib_bytes) {
                Ok(img) => img,
                Err(e) => {
                    tracing::debug!("Failed to decode DIB: {e}");
                    return Ok(None);
                }
            };
            let w = rgba.width() as i64;
            let h = rgba.height() as i64;
            let png_bytes = rgba_to_png(&rgba)?;

            return Ok(Some(Capture {
                kind: Kind::Image,
                sub_kind: None,
                primary: Some(png_bytes),
                formats: Vec::new(),
                files: Vec::new(),
                preview_text: None,
                ext: Some("PNG".into()),
                mime: Some("image/png".into()),
                width: Some(w),
                height: Some(h),
                duration_ms: None,
                source_app: None,
                is_reference: false,
                ref_path: None,
            }));
        }
    }

    Ok(None)
}

/// Decodes text formats (`HTML Format`, `Rich Text Format`, `CF_UNICODETEXT`).
fn decode_text(max_bytes: u64) -> AppResult<Option<Capture>> {
    let mut captured_formats = Vec::new();

    let has_html = is_format_available(*FORMAT_HTML);
    let has_rtf = is_format_available(*FORMAT_RTF);
    let has_unicode = is_format_available(CF_UNICODETEXT);
    let has_ansi = is_format_available(CF_TEXT);

    if !has_html && !has_rtf && !has_unicode && !has_ansi {
        return Ok(None);
    }

    if has_html {
        if let Some(bytes) = read_raw_clipboard_bytes(*FORMAT_HTML, max_bytes) {
            captured_formats.push(CapturedFormat {
                format: "HTML Format".into(),
                bytes,
            });
        }
    }

    if has_rtf {
        if let Some(bytes) = read_raw_clipboard_bytes(*FORMAT_RTF, max_bytes) {
            captured_formats.push(CapturedFormat {
                format: "Rich Text Format".into(),
                bytes,
            });
        }
    }

    // Get plain text canonical representation
    let plain_text = if has_unicode {
        read_clipboard_unicode_text(max_bytes)
    } else if has_ansi {
        read_clipboard_ansi_text(max_bytes)
    } else {
        None
    };

    let text_content = match plain_text {
        Some(t) if !t.is_empty() => t,
        _ => {
            // If no plain text format was present but HTML/RTF was, extract or return
            if captured_formats.is_empty() {
                return Ok(None);
            }
            "Rich Text Item".to_string()
        }
    };

    let mut sub_kind = classify::classify_text(&text_content);
    if !captured_formats.is_empty() && sub_kind == SubKind::Plain {
        sub_kind = SubKind::Rich;
    }

    let primary = text_content.as_bytes().to_vec();

    Ok(Some(Capture {
        kind: Kind::Text,
        sub_kind: Some(sub_kind),
        primary: Some(primary),
        formats: captured_formats,
        files: Vec::new(),
        preview_text: Some(text_content),
        ext: Some("TXT".into()),
        mime: Some("text/plain".into()),
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: false,
        ref_path: None,
    }))
}

/// Reads raw bytes for a given clipboard format.
pub fn read_raw_clipboard_bytes(format: u32, max_bytes: u64) -> Option<Vec<u8>> {
    if format == 0 {
        return None;
    }
    unsafe {
        // Sound: Caller holds open clipboard lock; retrieves handle for requested format ID.
        let handle = match GetClipboardData(format) {
            Ok(h) if !h.0.is_null() => h,
            _ => return None,
        };
        let hglobal = HGLOBAL(handle.0);
        // Sound: GlobalSize queries byte length of allocated clipboard memory.
        let size = GlobalSize(hglobal);
        if size == 0 || (size as u64) > max_bytes {
            return None;
        }
        // Sound: GlobalLock acquires pointer to global memory block valid until GlobalUnlock.
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        // Sound: ptr is valid for size bytes while locked by GlobalLock.
        let slice = std::slice::from_raw_parts(ptr as *const u8, size);
        let data = slice.to_vec();
        // Sound: GlobalUnlock releases lock on hglobal after copying data to owned Vec.
        let _ = GlobalUnlock(hglobal);
        Some(data)
    }
}

/// Reads UTF-16 string from `CF_UNICODETEXT`.
fn read_clipboard_unicode_text(max_bytes: u64) -> Option<String> {
    unsafe {
        // Sound: Caller holds open clipboard lock; retrieves handle for CF_UNICODETEXT.
        let handle = match GetClipboardData(CF_UNICODETEXT) {
            Ok(h) if !h.0.is_null() => h,
            _ => return None,
        };
        let hglobal = HGLOBAL(handle.0);
        // Sound: GlobalSize queries byte length of allocated clipboard memory block.
        let size = GlobalSize(hglobal);
        if size < 2 || (size as u64) > max_bytes {
            return None;
        }
        // Sound: GlobalLock acquires pointer to global memory block valid until GlobalUnlock.
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        let u16_len = size / 2;
        // Sound: ptr is valid for u16_len 16-bit elements while locked by GlobalLock.
        let slice = std::slice::from_raw_parts(ptr as *const u16, u16_len);
        let actual_len = slice.iter().position(|&c| c == 0).unwrap_or(u16_len);
        let text = String::from_utf16_lossy(&slice[..actual_len]);
        // Sound: GlobalUnlock releases lock on hglobal after converting string.
        let _ = GlobalUnlock(hglobal);
        Some(text)
    }
}

/// Reads ANSI string from `CF_TEXT`.
fn read_clipboard_ansi_text(max_bytes: u64) -> Option<String> {
    unsafe {
        // Sound: Caller holds open clipboard lock; retrieves handle for CF_TEXT.
        let handle = match GetClipboardData(CF_TEXT) {
            Ok(h) if !h.0.is_null() => h,
            _ => return None,
        };
        let hglobal = HGLOBAL(handle.0);
        // Sound: GlobalSize queries byte length of allocated clipboard memory block.
        let size = GlobalSize(hglobal);
        if size == 0 || (size as u64) > max_bytes {
            return None;
        }
        // Sound: GlobalLock acquires pointer to global memory block valid until GlobalUnlock.
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        // Sound: ptr is valid for size bytes while locked by GlobalLock.
        let slice = std::slice::from_raw_parts(ptr as *const u8, size);
        let actual_len = slice.iter().position(|&c| c == 0).unwrap_or(size);
        let text = String::from_utf8_lossy(&slice[..actual_len]).to_string();
        // Sound: GlobalUnlock releases lock on hglobal after converting string.
        let _ = GlobalUnlock(hglobal);
        Some(text)
    }
}

/// Converts `image::RgbaImage` to PNG encoded bytes.
pub fn rgba_to_png(img: &RgbaImage) -> AppResult<Vec<u8>> {
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    img.write_with_encoder(encoder)?;
    Ok(png_bytes)
}

/// Decodes DIB / DIBV5 byte buffer into an `image::RgbaImage`.
///
/// Supports:
/// - Top-down (`height < 0`) and bottom-up (`height > 0`) bitmaps
/// - 32-bit `BI_RGB` with alpha-channel presence detection (all-zero alpha -> opaque)
/// - 32-bit `BI_BITFIELDS` with arbitrary color masks
/// - 24-bit `BI_RGB`
/// - 16-bit `BI_RGB` (5-5-5) and `BI_BITFIELDS` (5-6-5)
/// - 8-bit, 4-bit, 1-bit palette indexed bitmaps
pub fn decode_dib_to_rgba(dib: &[u8]) -> AppResult<RgbaImage> {
    if dib.len() < 40 {
        return Err(AppError::Other("DIB buffer is too small for BITMAPINFOHEADER".into()));
    }

    let header_size = match read_u32_le(&dib[0..4]) {
        Some(sz) if (sz as usize) >= 40 && (sz as usize) <= dib.len() => sz as usize,
        _ => return Err(AppError::Other("Invalid DIB header size".into())),
    };
    let width_raw = match read_i32_le(&dib[4..8]) {
        Some(w) if w > 0 => w as u32,
        _ => return Err(AppError::Other("Invalid DIB width".into())),
    };
    let height_raw = match read_i32_le(&dib[8..12]) {
        Some(h) if h != 0 && h != i32::MIN => h,
        _ => return Err(AppError::Other("Invalid DIB height".into())),
    };
    let _planes = match read_u16_le(&dib[12..14]) {
        Some(p) if p == 1 => p,
        _ => return Err(AppError::Other("Invalid DIB plane count".into())),
    };
    let bit_count = match read_u16_le(&dib[14..16]) {
        Some(b) if matches!(b, 1 | 4 | 8 | 16 | 24 | 32) => b,
        _ => return Err(AppError::Other("Unsupported DIB bit count".into())),
    };
    let compression = read_u32_le(&dib[16..20]).unwrap_or(0);
    let clr_used = read_u32_le(&dib[32..36]).unwrap_or(0);

    let width = width_raw;
    let height = height_raw.unsigned_abs();
    let is_top_down = height_raw < 0;

    let mut mask_r = 0x00FF0000u32;
    let mut mask_g = 0x0000FF00u32;
    let mut mask_b = 0x000000FFu32;
    let mut mask_a = 0xFF000000u32;

    let mut data_offset = header_size;

    // Bitfield masks
    if compression == 3 /* BI_BITFIELDS */ || compression == 6 /* BI_ALPHABITFIELDS */ {
        if header_size == 40 {
            if dib.len() < 52 {
                return Err(AppError::Other("Truncated DIB bitfields".into()));
            }
            mask_r = read_u32_le(&dib[40..44]).unwrap_or(0);
            mask_g = read_u32_le(&dib[44..48]).unwrap_or(0);
            mask_b = read_u32_le(&dib[48..52]).unwrap_or(0);
            if dib.len() >= 56 && compression == 6 {
                mask_a = read_u32_le(&dib[52..56]).unwrap_or(0);
                data_offset = 56;
            } else {
                mask_a = !(mask_r | mask_g | mask_b);
                data_offset = 52;
            }
        } else if header_size >= 108 /* BITMAPV4HEADER or BITMAPV5HEADER */ {
            if dib.len() < 56 {
                return Err(AppError::Other("Truncated DIB V4/V5 header".into()));
            }
            mask_r = read_u32_le(&dib[40..44]).unwrap_or(0);
            mask_g = read_u32_le(&dib[44..48]).unwrap_or(0);
            mask_b = read_u32_le(&dib[48..52]).unwrap_or(0);
            mask_a = read_u32_le(&dib[52..56]).unwrap_or(0);
        }
    } else if bit_count == 16 {
        // Default 16-bit BI_RGB is 5-5-5
        mask_r = 0x7C00;
        mask_g = 0x03E0;
        mask_b = 0x001F;
        mask_a = 0x0000;
    }

    // Palette handling for indexed bitmaps (<= 8 bpp)
    let palette_colors = if bit_count <= 8 {
        let max_entries = 1usize << bit_count;
        let count = if clr_used > 0 {
            (clr_used as usize).min(max_entries)
        } else {
            max_entries
        };
        if count == 0 {
            return Err(AppError::Other("Zero-length palette in indexed DIB".into()));
        }
        let palette_bytes = match count.checked_mul(4) {
            Some(b) => b,
            None => return Err(AppError::Other("Palette size overflow".into())),
        };
        let pal_end = match data_offset.checked_add(palette_bytes) {
            Some(end) => end,
            None => return Err(AppError::Other("Palette offset overflow".into())),
        };
        if dib.len() < pal_end {
            return Err(AppError::Other("Truncated DIB color palette".into()));
        }
        let palette = &dib[data_offset..pal_end];
        data_offset = pal_end;
        Some(palette)
    } else {
        None
    };

    let bits_per_row = match (width as u64).checked_mul(bit_count as u64) {
        Some(bits) => bits,
        None => return Err(AppError::Other("DIB bits per row overflow".into())),
    };
    let row_stride = match bits_per_row.checked_add(31).map(|b| (b / 32) * 4) {
        Some(stride) if stride > 0 => stride as usize,
        _ => return Err(AppError::Other("Invalid DIB row stride".into())),
    };

    if data_offset > dib.len() {
        return Err(AppError::Other("DIB header exceeds buffer length".into()));
    }
    let pixel_data = &dib[data_offset..];

    // Protect against OOM panic from malicious or corrupt dimensions
    let total_pixels = match (width as u64).checked_mul(height as u64) {
        Some(px) => px,
        None => return Err(AppError::Other("DIB dimensions overflow pixel count".into())),
    };
    let total_rgba_bytes = match total_pixels.checked_mul(4) {
        Some(bytes) => bytes,
        None => return Err(AppError::Other("DIB dimensions overflow RGBA buffer size".into())),
    };
    if total_rgba_bytes > 256 * 1024 * 1024 {
        return Err(AppError::TooLarge(total_rgba_bytes));
    }

    let mut img = RgbaImage::new(width, height);

    match bit_count {
        32 => {
            // First pass: detect if any pixel has non-zero alpha
            let mut has_non_zero_alpha = false;
            let (r_shift, r_bits) = mask_shift_and_bits(mask_r);
            let (g_shift, g_bits) = mask_shift_and_bits(mask_g);
            let (b_shift, b_bits) = mask_shift_and_bits(mask_b);
            let (a_shift, a_bits) = mask_shift_and_bits(mask_a);

            if a_bits > 0 {
                for y in 0..height {
                    let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                    let row_start = match src_y.checked_mul(row_stride) {
                        Some(s) => s,
                        None => continue,
                    };
                    if row_start + (width as usize).saturating_mul(4) > pixel_data.len() {
                        continue;
                    }
                    for x in 0..width {
                        let px_start = row_start + (x as usize) * 4;
                        if px_start + 4 > pixel_data.len() {
                            break;
                        }
                        let val = read_u32_le(&pixel_data[px_start..px_start + 4]).unwrap_or(0);
                        let a_val = scale_bits((val & mask_a) >> a_shift, a_bits);
                        if a_val > 0 {
                            has_non_zero_alpha = true;
                            break;
                        }
                    }
                    if has_non_zero_alpha {
                        break;
                    }
                }
            }

            // Decode pixels
            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                if row_start + (width as usize).saturating_mul(4) > pixel_data.len() {
                    continue;
                }
                for x in 0..width {
                    let px_start = row_start + (x as usize) * 4;
                    if px_start + 4 > pixel_data.len() {
                        break;
                    }
                    let val = read_u32_le(&pixel_data[px_start..px_start + 4]).unwrap_or(0);

                    let r = scale_bits((val & mask_r) >> r_shift, r_bits);
                    let g = scale_bits((val & mask_g) >> g_shift, g_bits);
                    let b = scale_bits((val & mask_b) >> b_shift, b_bits);
                    let a = if has_non_zero_alpha && a_bits > 0 {
                        scale_bits((val & mask_a) >> a_shift, a_bits)
                    } else {
                        255
                    };

                    img.put_pixel(x, y, image::Rgba([r, g, b, a]));
                }
            }
        }
        24 => {
            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                if row_start + (width as usize).saturating_mul(3) > pixel_data.len() {
                    continue;
                }
                for x in 0..width {
                    let px_start = row_start + (x as usize) * 3;
                    if px_start + 3 > pixel_data.len() {
                        break;
                    }
                    let b = pixel_data[px_start];
                    let g = pixel_data[px_start + 1];
                    let r = pixel_data[px_start + 2];
                    img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                }
            }
        }
        16 => {
            let (r_shift, r_bits) = mask_shift_and_bits(mask_r);
            let (g_shift, g_bits) = mask_shift_and_bits(mask_g);
            let (b_shift, b_bits) = mask_shift_and_bits(mask_b);

            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                if row_start + (width as usize).saturating_mul(2) > pixel_data.len() {
                    continue;
                }
                for x in 0..width {
                    let px_start = row_start + (x as usize) * 2;
                    if px_start + 2 > pixel_data.len() {
                        break;
                    }
                    let val = read_u16_le(&pixel_data[px_start..px_start + 2]).unwrap_or(0) as u32;
                    let r = scale_bits((val & mask_r) >> r_shift, r_bits);
                    let g = scale_bits((val & mask_g) >> g_shift, g_bits);
                    let b = scale_bits((val & mask_b) >> b_shift, b_bits);
                    img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                }
            }
        }
        8 => {
            let Some(pal) = palette_colors else {
                return Err(AppError::Other("Missing palette for 8-bit DIB".into()));
            };
            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                if row_start + (width as usize) > pixel_data.len() {
                    continue;
                }
                for x in 0..width {
                    let px_idx = row_start + (x as usize);
                    if px_idx >= pixel_data.len() {
                        break;
                    }
                    let idx = pixel_data[px_idx] as usize;
                    let pal_offset = idx * 4;
                    if pal_offset + 3 < pal.len() {
                        let b = pal[pal_offset];
                        let g = pal[pal_offset + 1];
                        let r = pal[pal_offset + 2];
                        img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                    }
                }
            }
        }
        4 => {
            let Some(pal) = palette_colors else {
                return Err(AppError::Other("Missing palette for 4-bit DIB".into()));
            };
            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                for x in 0..width {
                    let byte_idx = row_start + (x as usize) / 2;
                    if byte_idx >= pixel_data.len() {
                        continue;
                    }
                    let byte_val = pixel_data[byte_idx];
                    let idx = if x % 2 == 0 {
                        (byte_val >> 4) as usize
                    } else {
                        (byte_val & 0x0F) as usize
                    };
                    let pal_offset = idx * 4;
                    if pal_offset + 3 < pal.len() {
                        let b = pal[pal_offset];
                        let g = pal[pal_offset + 1];
                        let r = pal[pal_offset + 2];
                        img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                    }
                }
            }
        }
        1 => {
            let Some(pal) = palette_colors else {
                return Err(AppError::Other("Missing palette for 1-bit DIB".into()));
            };
            for y in 0..height {
                let src_y = if is_top_down { y as usize } else { (height - 1 - y) as usize };
                let row_start = match src_y.checked_mul(row_stride) {
                    Some(s) => s,
                    None => continue,
                };
                for x in 0..width {
                    let byte_idx = row_start + (x as usize) / 8;
                    if byte_idx >= pixel_data.len() {
                        continue;
                    }
                    let bit = (pixel_data[byte_idx] >> (7 - (x % 8))) & 1;
                    let pal_offset = (bit as usize) * 4;
                    if pal_offset + 3 < pal.len() {
                        let b = pal[pal_offset];
                        let g = pal[pal_offset + 1];
                        let r = pal[pal_offset + 2];
                        img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                    }
                }
            }
        }
        _ => {
            return Err(AppError::Other(format!("Unsupported DIB bit count: {bit_count}")));
        }
    }

    Ok(img)
}

fn read_u16_le(b: &[u8]) -> Option<u16> {
    if b.len() < 2 {
        None
    } else {
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
}

fn read_u32_le(b: &[u8]) -> Option<u32> {
    if b.len() < 4 {
        None
    } else {
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn read_i32_le(b: &[u8]) -> Option<i32> {
    if b.len() < 4 {
        None
    } else {
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn mask_shift_and_bits(mask: u32) -> (u32, u32) {
    if mask == 0 {
        return (0, 0);
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    (shift, bits)
}

fn scale_bits(val: u32, bits: u32) -> u8 {
    if bits == 0 {
        return 0;
    }
    if bits == 8 {
        return (val & 0xFF) as u8;
    }
    let max_in = (1u32 << bits) - 1;
    if max_in == 0 {
        return 0;
    }
    ((val * 255 + max_in / 2) / max_in) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_dib_32bit_bottom_up() {
        let width = 2u32;
        let height = 2u32;
        let mut dib = Vec::new();

        // BITMAPINFOHEADER (40 bytes)
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
        dib.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
        dib.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight (bottom-up)
        dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
        dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
        dib.extend_from_slice(&(width * height * 4).to_le_bytes()); // biSizeImage
        dib.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
        dib.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
        dib.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        dib.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

        // Bottom row (y = 1 in image space): Red [0, 0, 255, 0], Green [0, 255, 0, 0]
        dib.extend_from_slice(&[0, 0, 255, 0]); // BGRA: Red
        dib.extend_from_slice(&[0, 255, 0, 0]); // BGRA: Green

        // Top row (y = 0 in image space): Blue [255, 0, 0, 0], White [255, 255, 255, 0]
        dib.extend_from_slice(&[255, 0, 0, 0]); // BGRA: Blue
        dib.extend_from_slice(&[255, 255, 255, 0]); // BGRA: White

        let img = decode_dib_to_rgba(&dib).expect("Should decode 32-bit DIB");
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);

        // Top row (y=0)
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([0, 0, 255, 255])); // Blue
        assert_eq!(img.get_pixel(1, 0), &image::Rgba([255, 255, 255, 255])); // White

        // Bottom row (y=1)
        assert_eq!(img.get_pixel(0, 1), &image::Rgba([255, 0, 0, 255])); // Red
        assert_eq!(img.get_pixel(1, 1), &image::Rgba([0, 255, 0, 255])); // Green
    }

    #[test]
    fn test_decode_dib_32bit_top_down() {
        let width = 2u32;
        let height = 2u32;
        let mut dib = Vec::new();

        // Header with negative height (top-down)
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(-(height as i32)).to_le_bytes()); // -2 (top-down)
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&(width * height * 4).to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());

        // Top row (y = 0): Blue, White
        dib.extend_from_slice(&[255, 0, 0, 0]);
        dib.extend_from_slice(&[255, 255, 255, 0]);

        // Bottom row (y = 1): Red, Green
        dib.extend_from_slice(&[0, 0, 255, 0]);
        dib.extend_from_slice(&[0, 255, 0, 0]);

        let img = decode_dib_to_rgba(&dib).expect("Should decode top-down 32-bit DIB");
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([0, 0, 255, 255]));
        assert_eq!(img.get_pixel(1, 0), &image::Rgba([255, 255, 255, 255]));
        assert_eq!(img.get_pixel(0, 1), &image::Rgba([255, 0, 0, 255]));
        assert_eq!(img.get_pixel(1, 1), &image::Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn test_decode_dib_24bit() {
        let width = 2u32;
        let height = 1u32;
        let mut dib = Vec::new();

        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(height as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes()); // 24-bit
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());

        // 2 pixels = 6 bytes + 2 bytes padding to align to 4 bytes = 8 bytes
        dib.extend_from_slice(&[0, 0, 255]); // BGR: Red
        dib.extend_from_slice(&[0, 255, 0]); // BGR: Green
        dib.extend_from_slice(&[0, 0]); // 2 bytes padding

        let img = decode_dib_to_rgba(&dib).expect("Should decode 24-bit DIB");
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));
        assert_eq!(img.get_pixel(1, 0), &image::Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn test_decode_dib_32bit_alpha_preservation() {
        let width = 1u32;
        let height = 1u32;
        let mut dib = Vec::new();

        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(height as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&4u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());

        // BGRA with 50% alpha (128)
        dib.extend_from_slice(&[100, 150, 200, 128]);

        let img = decode_dib_to_rgba(&dib).expect("Should decode 32-bit with alpha");
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([200, 150, 100, 128]));

        let png = rgba_to_png(&img).expect("Should encode to PNG");
        assert!(!png.is_empty());
        let loaded = image::load_from_memory(&png).expect("Should load encoded PNG");
        assert_eq!(loaded.width(), 1);
        assert_eq!(loaded.height(), 1);
    }

    #[test]
    fn test_decode_dib_8bit_palette() {
        let width = 2u32;
        let height = 1u32;
        let mut dib = Vec::new();

        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(width as i32).to_le_bytes());
        dib.extend_from_slice(&(height as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&8u16.to_le_bytes()); // 8-bit
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&2u32.to_le_bytes()); // 2 colors used
        dib.extend_from_slice(&0u32.to_le_bytes());

        // Palette entry 0: Blue [255, 0, 0, 0] (BGRA)
        dib.extend_from_slice(&[255, 0, 0, 0]);
        // Palette entry 1: Yellow [0, 255, 255, 0] (BGRA)
        dib.extend_from_slice(&[0, 255, 255, 0]);

        // Pixels: index 0, index 1 + 2 bytes padding to 4-byte boundary
        dib.extend_from_slice(&[0, 1, 0, 0]);

        let img = decode_dib_to_rgba(&dib).expect("Should decode 8-bit DIB");
        assert_eq!(img.get_pixel(0, 0), &image::Rgba([0, 0, 255, 255]));
        assert_eq!(img.get_pixel(1, 0), &image::Rgba([255, 255, 0, 255]));
    }

    #[test]
    fn test_corrupt_dib_8bpp_zero_palette() {
        let mut dib = Vec::new();
        // 40-byte header claiming 8bpp, but no palette following
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes()); // width 10
        dib.extend_from_slice(&10i32.to_le_bytes()); // height 10
        dib.extend_from_slice(&1u16.to_le_bytes()); // planes 1
        dib.extend_from_slice(&8u16.to_le_bytes()); // 8 bpp
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&100u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        // Truncated immediately after header: no palette
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_stride_shorter_than_row() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&100i32.to_le_bytes()); // width 100
        dib.extend_from_slice(&100i32.to_le_bytes()); // height 100
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes()); // 24 bpp -> row stride is 300 bytes
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&30000u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        // Only supply 10 bytes of pixel data instead of 30,000 bytes
        dib.extend_from_slice(&[0u8; 10]);
        // Must decode without panicking (unfilled pixels default to transparent/black)
        let res = decode_dib_to_rgba(&dib);
        assert!(res.is_ok());
    }

    #[test]
    fn test_corrupt_dib_negative_height_huge_width() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(i32::MAX).to_le_bytes()); // Huge width (overflow potential)
        dib.extend_from_slice(&(-10i32).to_le_bytes()); // Top-down
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        // Must fail gracefully on arithmetic/allocation overflow, never panic
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_bpp_zero() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&0u16.to_le_bytes()); // bpp 0
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_claimed_size_larger_than_buffer() {
        let mut dib = Vec::new();
        // Header claims size of 100 bytes, but buffer is only 40 bytes
        dib.extend_from_slice(&100u32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&400u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_negative_width() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(-10i32).to_le_bytes()); // Negative width is invalid in DIB
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&400u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_min_height() {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&10i32.to_le_bytes());
        dib.extend_from_slice(&(i32::MIN).to_le_bytes()); // i32::MIN causes negation overflow if not checked
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0i32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib).is_err());
    }

    #[test]
    fn test_corrupt_dib_4bpp_and_1bpp_missing_palette() {
        // 4bpp
        let mut dib4 = Vec::new();
        dib4.extend_from_slice(&40u32.to_le_bytes());
        dib4.extend_from_slice(&10i32.to_le_bytes());
        dib4.extend_from_slice(&10i32.to_le_bytes());
        dib4.extend_from_slice(&1u16.to_le_bytes());
        dib4.extend_from_slice(&4u16.to_le_bytes()); // 4bpp requires 16 palette entries
        dib4.extend_from_slice(&0u32.to_le_bytes());
        dib4.extend_from_slice(&0u32.to_le_bytes());
        dib4.extend_from_slice(&0i32.to_le_bytes());
        dib4.extend_from_slice(&0i32.to_le_bytes());
        dib4.extend_from_slice(&0u32.to_le_bytes());
        dib4.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib4).is_err());

        // 1bpp
        let mut dib1 = Vec::new();
        dib1.extend_from_slice(&40u32.to_le_bytes());
        dib1.extend_from_slice(&10i32.to_le_bytes());
        dib1.extend_from_slice(&10i32.to_le_bytes());
        dib1.extend_from_slice(&1u16.to_le_bytes());
        dib1.extend_from_slice(&1u16.to_le_bytes()); // 1bpp requires 2 palette entries
        dib1.extend_from_slice(&0u32.to_le_bytes());
        dib1.extend_from_slice(&0u32.to_le_bytes());
        dib1.extend_from_slice(&0i32.to_le_bytes());
        dib1.extend_from_slice(&0i32.to_le_bytes());
        dib1.extend_from_slice(&0u32.to_le_bytes());
        dib1.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_dib_to_rgba(&dib1).is_err());
    }
}
