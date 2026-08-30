//! Popup placement at the cursor, clamped to the work area of the monitor
//! under it, with per-monitor DPI applied *before* clamping.
//!
//! OWNER: worker W3.
//!
//! All coordinates here are physical pixels of the virtual screen, which is
//! what `GetCursorPos` and `GetMonitorInfoW` speak. The settings' "fixed" size
//! is logical, so it is scaled by the target monitor's DPI before clamping;
//! the "percent" size is a fraction of the work area and needs no scaling.

use tauri::WebviewWindow;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
};

use crate::error::{AppError, AppResult};
use crate::settings::WindowSettings;

/// Sizes the popup per `settings.window` and places its top-left at the cursor,
/// clamped so the whole window fits inside the work area of the monitor under
/// the cursor. Never lands above the work area's left/top edges.
pub fn place_popup(window: &WebviewWindow, ws: &WindowSettings) -> AppResult<()> {
    let hwnd = window.hwnd().map_err(tauri_err)?;
    let pt = cursor_point()?;
    // FFI: MonitorFromPoint never fails and returns a valid HMONITOR.
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let (work, scale) = work_area(monitor)?;

    let (w, h) = window_size(ws, &work, scale);

    // Top-left at the cursor, then clamp. Order matters: clamp against the
    // right/bottom edges first, then pin to left/top.
    let mut x = pt.x;
    let mut y = pt.y;
    if x + w as i32 > work.right {
        x = work.right - w as i32;
    }
    if y + h as i32 > work.bottom {
        y = work.bottom - h as i32;
    }
    if x < work.left {
        x = work.left;
    }
    if y < work.top {
        y = work.top;
    }

    // SetWindowPos with physical pixels; SWP_NOACTIVATE so focus management
    // stays in `show_popup`.
    // FFI: hwnd/x/y/w/h are all derived from live Win32 state in this call.
    unsafe {
        SetWindowPos(hwnd, None, x, y, w as i32, h as i32, SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| AppError::Win(format!("SetWindowPos: {e}")))?;
    }
    Ok(())
}

/// Centers the window on the work area of the monitor under the cursor,
/// preserving its current logical size. Used by the settings window.
pub fn center_on_cursor_monitor(window: &WebviewWindow) -> AppResult<()> {
    let hwnd = window.hwnd().map_err(tauri_err)?;
    let pt = cursor_point()?;
    // FFI: MonitorFromPoint never fails and returns a valid HMONITOR.
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let (work, scale) = work_area(monitor)?;

    // The window reports its size in physical pixels for *its current* monitor;
    // rescale to the target monitor so 960x660 stays 960x660 logical.
    let size = window.inner_size().map_err(tauri_err)?;
    let current = window.scale_factor().map_err(tauri_err)?;
    let w = (size.width as f64 / current * scale).round() as i32;
    let h = (size.height as f64 / current * scale).round() as i32;
    let x = work.left + ((work.right - work.left) - w) / 2;
    let y = work.top + ((work.bottom - work.top) - h) / 2;

    // FFI: hwnd is live and all coordinates are freshly computed physical px.
    unsafe {
        SetWindowPos(hwnd, None, x, y, w.max(1), h.max(1), SWP_NOZORDER | SWP_NOACTIVATE)
            .map_err(|e| AppError::Win(format!("SetWindowPos: {e}")))?;
    }
    Ok(())
}

fn cursor_point() -> AppResult<POINT> {
    let mut pt = POINT::default();
    // FFI: pt is a valid stack buffer; failure is reported via the Result.
    unsafe {
        GetCursorPos(&mut pt).map_err(|e| AppError::Win(format!("GetCursorPos: {e}")))?;
    }
    Ok(pt)
}

/// Work area of `monitor` in physical pixels plus its scale factor (DPI/96).
fn work_area(monitor: HMONITOR) -> AppResult<(RECT, f64)> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT::default(),
        rcWork: RECT::default(),
        dwFlags: 0,
    };
    // cbSize must be set before the call or GetMonitorInfoW fails.
    // FFI: mi is a valid stack buffer with cbSize set; the BOOL result is checked.
    let ok = unsafe { GetMonitorInfoW(monitor, &mut mi) };
    if !ok.as_bool() {
        return Err(AppError::Win("GetMonitorInfoW failed".into()));
    }
    let mut dpix = 0u32;
    let mut dpiy = 0u32;
    // FFI: out-params are valid; failure just falls back to 1.0 scale.
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpix, &mut dpiy) };
    let scale = if dpix > 0 { dpix as f64 / 96.0 } else { 1.0 };
    Ok((mi.rcWork, scale))
}

fn window_size(ws: &WindowSettings, work: &RECT, scale: f64) -> (u32, u32) {
    let work_w = (work.right - work.left).max(1) as f64;
    let work_h = (work.bottom - work.top).max(1) as f64;
    match ws.size_mode.as_str() {
        "fixed" => {
            let w = (ws.fixed.width as f64 * scale).round().max(1.0) as u32;
            let h = (ws.fixed.height as f64 * scale).round().max(1.0) as u32;
            (w, h)
        }
        _ => {
            let pct = ws.percent_of_monitor.clamp(1, 100) as f64 / 100.0;
            (
                (work_w * pct).round().max(1.0) as u32,
                (work_h * pct).round().max(1.0) as u32,
            )
        }
    }
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}