//! Windows shell integration for the context menu and drag-out.
//!
//! OWNER: worker W4. `reveal` uses `SHOpenFolderAndSelectItems` rather than
//! `explorer /select` so it reuses an existing Explorer window; `open_with`
//! uses `SHOpenWithDialog`.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    SHOpenFolderAndSelectItems, SHOpenWithDialog, SHParseDisplayName, ShellExecuteW, OPENASINFO,
    OPEN_AS_INFO_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::error::{AppError, AppResult};
use crate::store::Store;

/// NUL-terminated UTF-16, for `PCWSTR` parameters.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// `SHOpenWithDialog` and `SHOpenFolderAndSelectItems` need COM initialized on
/// the calling thread, or they fail intermittently and only on some machines.
/// Initialize STA per call, uninitialize on the way out.
struct ComScope;

impl ComScope {
    fn init() -> AppResult<Self> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if hr.is_err() {
                return Err(AppError::Win(format!("CoInitializeEx failed: {hr}")));
            }
        }
        Ok(ComScope)
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

/// Opens a file or directory with its default handler. Works on stored blob
/// files and on reference paths alike.
pub fn open(path: &Path) -> AppResult<()> {
    let p = wide(path.as_os_str());
    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(p.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        let code = result.0 as isize;
        if code <= 32 {
            return Err(AppError::Win(format!("ShellExecuteW failed: {code}")));
        }
    }
    Ok(())
}

/// Opens the shell's "Open with…" dialog for a file.
pub fn open_with(path: &Path) -> AppResult<()> {
    let _com = ComScope::init()?;
    let p = wide(path.as_os_str());
    let info = OPENASINFO {
        pcszFile: PCWSTR(p.as_ptr()),
        pcszClass: PCWSTR::null(),
        oaifInFlags: OPEN_AS_INFO_FLAGS(0),
    };
    unsafe {
        SHOpenWithDialog(None, &info)
            .map_err(|e| AppError::Win(format!("SHOpenWithDialog failed: {e}")))
    }
}

/// Reveals a file in Explorer, selecting it, or opens the folder itself when
/// given a directory. Uses `SHOpenFolderAndSelectItems` (not
/// `explorer /select`) so an existing Explorer window is reused.
pub fn reveal(path: &Path) -> AppResult<()> {
    let _com = ComScope::init()?;
    unsafe {
        let item_wide = wide(path.as_os_str());
        let mut item_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(item_wide.as_ptr()), None, &mut item_pidl, 0, None)
            .map_err(|e| AppError::Win(format!("SHParseDisplayName failed: {e}")))?;
        if item_pidl.is_null() {
            return Err(AppError::Win("SHParseDisplayName returned no pidl".into()));
        }

        let result = if path.is_dir() {
            SHOpenFolderAndSelectItems(item_pidl, None, 0)
        } else {
            let parent = path.parent().unwrap_or(path);
            let parent_wide = wide(parent.as_os_str());
            let mut parent_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(parent_wide.as_ptr()), None, &mut parent_pidl, 0, None)
                .map_err(|e| AppError::Win(format!("SHParseDisplayName failed: {e}")))?;
            let result = SHOpenFolderAndSelectItems(parent_pidl, Some(&[item_pidl]), 0);
            if !parent_pidl.is_null() {
                CoTaskMemFree(Some(parent_pidl as *const std::ffi::c_void));
            }
            result
        };

        if !item_pidl.is_null() {
            CoTaskMemFree(Some(item_pidl as *const std::ffi::c_void));
        }
        result.map_err(|e| AppError::Win(format!("SHOpenFolderAndSelectItems failed: {e}")))
    }
}

/// Phase 5. Materializes non-file items to temp files, then starts an OLE drag
/// carrying `CF_HDROP`. Explicitly out of scope for the W4 platform task.
pub fn begin_drag(_store: &Store, _ids: &[i64]) -> AppResult<()> {
    todo!("W4 — phase 5")
}