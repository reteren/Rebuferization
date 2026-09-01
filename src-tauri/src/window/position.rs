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

/// Sizes the popup per `settings.window` and CENTERS it on the cursor, clamped
/// so the whole window fits inside the work area of the monitor under the
/// cursor. Never lands above the work area's left/top edges.
pub fn place_popup(window: &WebviewWindow, ws: &WindowSettings) -> AppResult<()> {
    let hwnd = window.hwnd().map_err(tauri_err)?;
    let pt = cursor_point()?;
    // FFI: MonitorFromPoint never fails and returns a valid HMONITOR.
    let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let (work, scale) = work_area(monitor)?;

    let (w, h) = window_size(ws, &work, scale);
    // Clamp the size to the work area before positioning: a fixed-size window
    // wider than the monitor would otherwise overflow past the right/bottom
    // edge — onto the adjacent monitor or off-screen — because the position
    // clamp lets the left/top edge win.
    let (w, h) = clamp_size_to_work(w, h, &work);

    // Centered on the cursor, then clamped. Putting the top-left corner at the
    // cursor meant the pointer landed in the window's corner and the content
    // opened down-right of it; centering puts what you are pointing at under
    // the pointer.
    let (x, y) = clamp_pos_to_work(
        pt.x - w as i32 / 2,
        pt.y - h as i32 / 2,
        w as i32,
        h as i32,
        &work,
    );

    // SetWindowPos with physical pixels; SWP_NOACTIVATE so focus management
    // stays in `show_popup`.
    // FFI: hwnd/x/y/w/h are all derived from live Win32 state in this call.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            w as i32,
            h as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
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
    // rescale to the target monitor so 960x660 stays 960x660 logical. The size
    // is then clamped to the work area so a small monitor never gets a window
    // that hangs off its edges (the centering formula is in-bounds once the
    // size fits).
    let size = window.inner_size().map_err(tauri_err)?;
    let current = window.scale_factor().map_err(tauri_err)?;
    let w = (size.width as f64 / current * scale).round() as i32;
    let h = (size.height as f64 / current * scale).round() as i32;
    let (w, h) = clamp_size_to_work(w.max(1) as u32, h.max(1) as u32, &work);
    let x = work.left + ((work.right - work.left) - w as i32) / 2;
    let y = work.top + ((work.bottom - work.top) - h as i32) / 2;

    // FFI: hwnd is live and all coordinates are freshly computed physical px.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            w as i32,
            h as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
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

/// Clamps a requested size so the window never exceeds the work area of its
/// monitor. Must run before the position clamp: once the size fits, the
/// right/bottom-then-left/top position clamp can no longer push the window
/// past an edge.
fn clamp_size_to_work(w: u32, h: u32, work: &RECT) -> (u32, u32) {
    let work_w = (work.right - work.left).max(1) as u32;
    let work_h = (work.bottom - work.top).max(1) as u32;
    (w.min(work_w), h.min(work_h))
}

/// Clamps a top-left position so the whole `w x h` rect sits inside the work
/// area. Right/bottom edges first, then pinned to left/top; with a size that
/// already fits, the result is always fully inside.
fn clamp_pos_to_work(x: i32, y: i32, w: i32, h: i32, work: &RECT) -> (i32, i32) {
    let mut x = x;
    let mut y = y;
    if x + w > work.right {
        x = work.right - w;
    }
    if y + h > work.bottom {
        y = work.bottom - h;
    }
    if x < work.left {
        x = work.left;
    }
    if y < work.top {
        y = work.top;
    }
    (x, y)
}

fn tauri_err(e: tauri::Error) -> AppError {
    AppError::Other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn size_clamps_to_the_work_area() {
        // A 2500x1400 fixed window on a 1920x1040 work area must shrink to fit.
        let work = rect(0, 0, 1920, 1040);
        assert_eq!(clamp_size_to_work(2500, 1400, &work), (1920, 1040));
        // A window smaller than the work area is untouched.
        assert_eq!(clamp_size_to_work(768, 416, &work), (768, 416));
        // Same on a monitor left of the primary (negative coordinates).
        let left_work = rect(-1920, 0, 0, 1040);
        assert_eq!(clamp_size_to_work(2500, 1400, &left_work), (1920, 1040));
        // A zero-size work area still yields at least 1x1.
        assert_eq!(clamp_size_to_work(800, 600, &rect(0, 0, 0, 0)), (1, 1));
    }

    #[test]
    fn position_clamps_inside_the_work_area() {
        let work = rect(0, 0, 1920, 1040);
        // Cursor at the extreme bottom-right: window slides up and left.
        assert_eq!(clamp_pos_to_work(1919, 1039, 768, 416, &work), (1152, 624));
        // Cursor near the top-left: unchanged.
        assert_eq!(clamp_pos_to_work(10, 10, 768, 416, &work), (10, 10));
    }

    #[test]
    fn position_clamps_on_a_monitor_left_of_the_primary() {
        // Secondary monitor to the LEFT: negative coordinates.
        let work = rect(-1920, 0, 0, 1040);
        // Middle of the monitor: unchanged, fully inside.
        assert_eq!(clamp_pos_to_work(-950, 540, 768, 416, &work), (-950, 540));
        // Extreme bottom-right of that monitor: clamped up/left, still negative.
        assert_eq!(clamp_pos_to_work(-10, 1038, 768, 416, &work), (-768, 624));
        // Cursor at the far left edge: pinned to the work area's left edge.
        assert_eq!(clamp_pos_to_work(-1920, 500, 768, 416, &work), (-1920, 500));
    }

    #[test]
    fn position_clamps_with_a_left_edge_taskbar() {
        // Taskbar on the left edge: the work area starts at x=48.
        let work = rect(48, 0, 1920, 1040);
        // Cursor over the taskbar: pinned up to the work area's left edge.
        assert_eq!(clamp_pos_to_work(10, 500, 768, 416, &work), (48, 500));
    }

    #[test]
    fn oversized_window_stays_on_its_monitor() {
        // Fixed-size window clamped to the work area first; the position then
        // never overflows onto the adjacent monitor to the right.
        let work = rect(0, 0, 1366, 768);
        let (w, h) = clamp_size_to_work(1440, 900, &work);
        assert_eq!((w, h), (1366, 768));
        let (x, y) = clamp_pos_to_work(1360, 760, w as i32, h as i32, &work);
        assert_eq!((x, y), (0, 0));
        assert!(x + w as i32 <= work.right && y + h as i32 <= work.bottom);
    }
}
