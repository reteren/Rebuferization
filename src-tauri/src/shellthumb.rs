//! Windows shell thumbnail extraction for formats the application cannot decode.

use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, SIZE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
    SIIGBF_THUMBNAILONLY,
};

use crate::error::{AppError, AppResult};

/// NUL-terminated UTF-16 for shell APIs.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Initializes COM for this call and only uninitializes when this scope owns
/// the initialization. A caller may already have selected an incompatible
/// apartment; COM then reports RPC_E_CHANGED_MODE, but the existing apartment
/// is still usable for the shell interfaces we need.
struct ComScope {
    initialized: bool,
}

impl ComScope {
    fn init() -> AppResult<Self> {
        // FFI: this initializes COM on the current thread, and Drop balances
        // it only when this call actually acquired a COM initialization count.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            Ok(Self { initialized: false })
        } else if result.is_err() {
            Err(AppError::Win(format!("CoInitializeEx failed: {result}")))
        } else {
            Ok(Self { initialized: true })
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialized {
            // FFI: this balances the successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() }
        }
    }
}

/// Owns the HBITMAP returned by IShellItemImageFactory.
///
/// COM manages the shell interfaces, but the bitmap is a GDI allocation whose
/// ownership is transferred to the caller and therefore needs its own guard.
struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        // FFI: the handle came from GetImage and remains valid until this
        // final DeleteObject, including while error paths unwind.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

/// Asks the Windows shell for the thumbnail it would show for `path`, at
/// most `max_edge` pixels on the long side. Returns the pixels as RGBA8
/// together with their width and height, or `None` when the shell has no
/// thumbnail for that file.
pub fn thumbnail_rgba(path: &Path, max_edge: u32) -> AppResult<Option<(Vec<u8>, u32, u32)>> {
    let _com = ComScope::init()?;
    let path_wide = wide(path.as_os_str());

    // FFI: the UTF-16 buffer is NUL-terminated and stays alive while the shell
    // item is created; the windows crate owns the returned COM interface.
    let item: IShellItem = unsafe {
        SHCreateItemFromParsingName(PCWSTR(path_wide.as_ptr()), None).map_err(|error| {
            AppError::Win(format!("SHCreateItemFromParsingName failed: {error}"))
        })?
    };
    let factory: IShellItemImageFactory = item
        .cast()
        .map_err(|error| AppError::Win(format!("IShellItemImageFactory cast failed: {error}")))?;

    let edge = max_edge.min(i32::MAX as u32) as i32;
    let size = SIZE { cx: edge, cy: edge };
    // FFI: GetImage receives a valid shell item factory and returns an owned
    // HBITMAP; OwnedBitmap releases that handle on success and every failure.
    let bitmap = match unsafe { factory.GetImage(size, SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY) }
    {
        Ok(bitmap) => OwnedBitmap(bitmap),
        Err(_) => return Ok(None),
    };

    let mut dimensions = BITMAP::default();
    // FFI: dimensions points to writable storage large enough for BITMAP, and
    // the HGDIOBJ is the HBITMAP returned by the shell.
    let copied = unsafe {
        GetObjectW(
            bitmap.0.into(),
            size_of::<BITMAP>() as i32,
            Some(&mut dimensions as *mut BITMAP as *mut std::ffi::c_void),
        )
    };
    if copied != size_of::<BITMAP>() as i32 || dimensions.bmWidth <= 0 || dimensions.bmHeight <= 0 {
        return Err(AppError::Win(
            "GetObjectW returned invalid bitmap dimensions".into(),
        ));
    }

    let width = dimensions.bmWidth as u32;
    let height = dimensions.bmHeight as u32;
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| AppError::Other("shell thumbnail is too large".into()))?;
    let mut bgra = Vec::new();
    bgra.try_reserve_exact(pixels)
        .map_err(|error| AppError::Other(format!("could not allocate shell thumbnail: {error}")))?;
    bgra.resize(pixels, 0);

    // GetDIBits needs a DC even though it only reads the bitmap. Keep the DC
    // lifetime explicit so it is released before the bitmap guard runs.
    // FFI: GetDC returns the screen DC for a null window handle, which is valid
    // for this read-only GDI conversion.
    let dc = unsafe { GetDC(None) };
    if dc.0.is_null() {
        return Err(AppError::Win(
            "GetDC failed while reading shell thumbnail".into(),
        ));
    }

    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: dimensions.bmWidth,
            // A negative height requests top-down rows, avoiding a later
            // vertical flip before the bytes are returned to the frontend.
            biHeight: -dimensions.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..BITMAPINFOHEADER::default()
        },
        ..BITMAPINFO::default()
    };
    // FFI: dc, bitmap, info, and bgra all remain valid for the duration of the
    // synchronous GetDIBits call, and the buffer has exactly four bytes/pixel.
    let copied_lines = unsafe {
        GetDIBits(
            dc,
            bitmap.0,
            0,
            height,
            Some(bgra.as_mut_ptr() as *mut std::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    // FFI: dc was obtained from GetDC(None) above and must be released on this
    // thread after the synchronous read completes.
    unsafe {
        ReleaseDC(None, dc);
    }
    if copied_lines == 0 {
        return Err(AppError::Win(
            "GetDIBits failed while reading shell thumbnail".into(),
        ));
    }

    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Some((bgra, width, height)))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::thumbnail_rgba;

    #[test]
    fn nonexistent_path_does_not_panic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.mp4");
        let _ = thumbnail_rgba(&path, 128);
    }

    #[test]
    fn directory_does_not_panic() {
        let dir = tempdir().unwrap();
        let _ = thumbnail_rgba(dir.path(), 128);
    }
}
