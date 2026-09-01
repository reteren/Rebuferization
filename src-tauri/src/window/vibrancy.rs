//! Window backdrop: acrylic on Windows 11, a flat dark background on
//! Windows 10.
//!
//! OWNER: worker W3.
//!
//! The version is detected from the NT build number via `RtlGetVersion`
//! (resolved with `GetProcAddress` so no Cargo feature is needed), not from
//! whether an API happens to fail. `GetVersionEx` lies under compat shims;
//! `RtlGetVersion` does not.

use tauri::WebviewWindow;
use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use crate::error::{AppError, AppResult};

/// `rgba(18, 20, 24, 0.92)` — the fallback backdrop and the acrylic tint.
const BACKDROP: (u8, u8, u8, u8) = (18, 20, 24, 235);

/// Windows 11 starts at NT build 22000; Windows 10 tops out at 19045.
const WINDOWS_11_BUILD: u32 = 22000;

/// Applies the backdrop to an already transparent, undecorated window.
/// `window-vibrancy` needs the window handle, which it obtains itself.
pub fn apply(window: &WebviewWindow) -> AppResult<()> {
    if windows_11_or_newer() {
        match window_vibrancy::apply_acrylic(window, Some(BACKDROP)) {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!("apply_acrylic failed ({e}); using the flat backdrop");
                set_fallback(window)?;
            }
        }
    } else {
        set_fallback(window)?;
    }
    Ok(())
}

/// Solid `rgba(18,20,24,0.92)` behind the transparent WebView.
fn set_fallback(window: &WebviewWindow) -> AppResult<()> {
    window
        .set_background_color(Some(tauri::window::Color(
            BACKDROP.0, BACKDROP.1, BACKDROP.2, BACKDROP.3,
        )))
        .map_err(|e| AppError::Other(format!("set_background_color: {e}")))?;
    Ok(())
}

/// True when the running NT kernel is Windows 11 or newer. On a detection
/// failure the safe (flat) fallback is chosen instead of guessing.
fn windows_11_or_newer() -> bool {
    build_number().is_some_and(|build| build >= WINDOWS_11_BUILD)
}

/// Real NT build number via `RtlGetVersion`, which ignores manifest compat
/// overrides.
fn build_number() -> Option<u32> {
    // SAFETY: all handles and buffers below are local and lifetime-contained:
    // ntdll is a module handle, proc is a plain export pointer, and the info
    // struct is a correctly sized, aligned stack buffer for RtlGetVersion.
    unsafe {
        let ntdll = GetModuleHandleW(w!("ntdll.dll")).ok()?;
        let proc = GetProcAddress(ntdll, s!("RtlGetVersion"));
        proc?;
        // FARPROC is a plain function pointer slot; the cast to the typed
        // signature is the documented way to call a resolved export.
        let f: RtlGetVersion = std::mem::transmute(proc);
        let mut info = std::mem::zeroed::<OsVersionInfoW>();
        info.size = std::mem::size_of::<OsVersionInfoW>() as u32;
        let status = f(&mut info);
        (status == 0).then_some(info.build)
    }
}

/// Layout matches the public `OSVERSIONINFOW`; `RtlGetVersion` fills the
/// fields up to the declared size.
#[repr(C)]
struct OsVersionInfoW {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
    csd: [u16; 128],
}

type RtlGetVersion = unsafe extern "system" fn(*mut OsVersionInfoW) -> i32;
