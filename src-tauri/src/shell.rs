//! Windows shell integration: the context menu, drag-out, and reveal.
//!
//! OWNER: worker W4. `reveal` uses `SHOpenFolderAndSelectItems` rather than
//! `explorer /select` so it reuses an existing Explorer window; `open_with`
//! uses `SHOpenWithDialog`. `begin_drag` runs an OLE drag carrying `CF_HDROP`,
//! materializing captured blobs to temp files with sensible names first.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use windows::core::{implement, BOOL, HRESULT, PCWSTR};
// In windows 0.61 the E_* HRESULT constants live under Win32::Foundation, not
// windows::core.
use windows::Win32::Foundation::{E_NOTIMPL, E_OUTOFMEMORY, E_POINTER};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, GlobalFree,
    OLE_E_ADVISENOTSUPPORTED, S_FALSE,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED, DATADIR_GET,
    DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC,
    IEnumFORMATETC_Impl, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    CF_HDROP, DoDragDrop, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropSource,
    IDropSource_Impl, OleInitialize, OleUninitialize,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DROPFILES, SHOpenFolderAndSelectItems, SHOpenWithDialog, SHParseDisplayName, ShellExecuteW,
    OPENASINFO, OPEN_AS_INFO_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::error::{AppError, AppResult};
use crate::model::{ItemDto, Kind};
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

/// OLE, not just COM. `DoDragDrop` needs the OLE subsystem — drag-and-drop and
/// the OLE clipboard live there — and `CoInitializeEx` alone does not start it:
/// the drag runs, every drop comes back `DROPEFFECT_NONE`, and the calls fail
/// intermittently with `CO_E_NOTINITIALIZED`. `OleInitialize` initializes COM
/// as a single-threaded apartment as well, so it replaces `ComScope` here
/// rather than sitting alongside it.
struct OleScope;

impl OleScope {
    fn init() -> AppResult<Self> {
        // FFI: OleInitialize takes a reserved null pointer and is safe to call
        // once per thread; the matching OleUninitialize runs in Drop.
        unsafe {
            OleInitialize(None).map_err(|e| AppError::Win(format!("OleInitialize failed: {e}")))?;
        }
        Ok(OleScope)
    }
}

impl Drop for OleScope {
    fn drop(&mut self) {
        // FFI: balances the OleInitialize above on this same thread.
        unsafe { OleUninitialize() }
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

/// Opens one stored item with its default handler.
///
/// It must go through `resolve_paths`, not the raw blob: a captured blob is
/// content-addressed, so its file name is a hash with no extension, and
/// ShellExecute on an extensionless file has nothing to dispatch on — Windows
/// answers with the "how do you want to open this?" picker every time. The
/// resolver materializes a temp copy named from the item's title and
/// extension, and hands references their original path untouched.
pub fn open_item(store: &Store, id: i64) -> AppResult<()> {
    let paths = resolve_paths(store, &[id])?;
    let path = paths
        .first()
        .ok_or_else(|| AppError::Other(format!("item {id} resolved to no file")))?;
    open(path)
}

/// Same resolution, then the shell's "Open with…" picker — which is where that
/// picker belongs, rather than appearing for an ordinary open.
pub fn open_item_with(store: &Store, id: i64) -> AppResult<()> {
    let paths = resolve_paths(store, &[id])?;
    let path = paths
        .first()
        .ok_or_else(|| AppError::Other(format!("item {id} resolved to no file")))?;
    open_with(path)
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

// ---------------------------------------------------------------------------
// Drag out (phase 5)
// ---------------------------------------------------------------------------

/// The per-process drag scratch folder. Materialized blobs live here for the
/// whole session and are never deleted while the app runs: Explorer copies the
/// files asynchronously after the drop, so deleting them when the drag ends
/// would corrupt the copy. The whole folder is wiped on first use instead —
/// the same "clean leftovers from a previous, possibly crashed run" idea as
/// the store's startup integrity sweep.
static SESSION_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
/// item id → materialized temp file, so dragging the same item twice reuses
/// the file instead of copying the blob again.
static SESSION_CACHE: Mutex<Option<HashMap<i64, PathBuf>>> = Mutex::new(None);

/// Resolves the session scratch directory, cleaning stale materialized files
/// from a previous run the first time it is used this session.
fn session_dir() -> AppResult<PathBuf> {
    let mut guard = SESSION_DIR.lock();
    if let Some(dir) = guard.as_ref() {
        return Ok(dir.clone());
    }
    let dir = std::env::temp_dir().join("rebuffer-drag");
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)?;
    *guard = Some(dir.clone());
    Ok(dir)
}

/// Resolves every id to a path the drop target can read: a reference drags its
/// original file, a captured item drags a materialized copy of its blob.
/// Missing blobs and gone reference files are errors the frontend can show,
/// never panics.
fn resolve_paths(store: &Store, ids: &[i64]) -> AppResult<Vec<PathBuf>> {
    if ids.is_empty() {
        return Err(AppError::Other("nothing to drag".into()));
    }
    let mut paths = Vec::with_capacity(ids.len());
    for &id in ids {
        let item = store.get(id)?;
        let path = if item.is_reference {
            let p = item.ref_path.ok_or_else(|| {
                AppError::Other(format!("item {id} is a reference but has no path"))
            })?;
            let pb = PathBuf::from(&p);
            if !pb.exists() {
                return Err(AppError::Other(format!(
                    "referenced file is gone: {}",
                    pb.display()
                )));
            }
            pb
        } else {
            // A captured item's blob is content-addressed (`blobs/ab/cd/<hash>`)
            // with a meaningless name, so it must be materialized under a
            // sensible name before it can be handed to a drop target.
            let blob = match store.blob_path(id) {
                Ok(p) => p,
                Err(AppError::NotFound(_)) => {
                    return Err(AppError::Other(format!(
                        "item {id} has no stored file to drag"
                    )));
                }
                Err(e) => return Err(e),
            };
            if !blob.exists() {
                return Err(AppError::Other(format!(
                    "stored file is missing for item {id}"
                )));
            }
            materialize_blob(&blob, &item)?
        };
        paths.push(path);
    }
    Ok(paths)
}

/// Materializes a blob into the session scratch folder, reusing the file when
/// the same item is dragged again.
fn materialize_blob(blob: &Path, item: &ItemDto) -> AppResult<PathBuf> {
    let dir = session_dir()?;
    let mut guard = SESSION_CACHE.lock();
    // HashMap::new is not const, so the static holds None until first use.
    let cache = guard.get_or_insert_with(HashMap::new);
    materialize_into(&dir, cache, blob, item)
}

/// The pure core of `materialize_blob`, factored out so it can be unit-tested
/// with a scratch dir instead of the real session folder.
fn materialize_into(
    dir: &Path,
    cache: &mut HashMap<i64, PathBuf>,
    blob: &Path,
    item: &ItemDto,
) -> AppResult<PathBuf> {
    if let Some(existing) = cache.get(&item.id) {
        return Ok(existing.clone());
    }
    let name = temp_file_name(item);
    let mut candidate = dir.join(&name);
    let mut n = 1u32;
    while candidate.exists() {
        candidate = dir.join(suffixed(&name, n));
        n += 1;
    }
    std::fs::copy(blob, &candidate)?;
    cache.insert(item.id, candidate.clone());
    Ok(candidate)
}

/// A sensible filename for a materialized blob: the sanitized `title` when
/// there is one, else a derived name (first line of the preview, or a
/// date-stamped fallback), plus the item's extension lowercased.
fn temp_file_name(item: &ItemDto) -> String {
    temp_file_name_on(item, &today())
}

/// `temp_file_name` with the date injected, so the fallback is testable.
fn temp_file_name_on(item: &ItemDto, date: &str) -> String {
    let base = item
        .title
        .as_deref()
        .map(sanitize_stem)
        .filter(|s| !s.is_empty())
        .or_else(|| derive_base(item, date))
        .unwrap_or_else(|| format!("rebuffer-{date}"));
    match item.ext.as_deref().map(|e| e.to_ascii_lowercase()) {
        Some(ext) if !ext.is_empty() => format!("{base}.{ext}"),
        _ => base,
    }
}

fn derive_base(item: &ItemDto, date: &str) -> Option<String> {
    if let Some(text) = item.preview_text.as_deref() {
        let first_line = text.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            let s = sanitize_stem(first_line);
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    if item.kind == Kind::Image {
        return Some(format!("screenshot-{date}"));
    }
    None
}

/// Replaces characters Windows forbids in a filename with `_`, trims trailing
/// dots and spaces, neutralizes reserved device names (`CON`, `NUL`, …), and
/// caps the length.
fn sanitize_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .take(80)
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut out = cleaned.trim_end_matches([' ', '.']).to_string();
    if is_reserved_name(out.split('.').next().unwrap_or("")) {
        out.insert(0, '_');
    }
    out
}

fn is_reserved_name(stem: &str) -> bool {
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
            | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
            | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    )
}

/// `foo.png`, 1 → `foo-1.png`; `foo`, 1 → `foo-1`. Used when two different
/// items would materialize to the same name.
fn suffixed(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
        None => format!("{name}-{n}"),
    }
}

/// Today as `YYYY-MM-DD`, used in derived temp names.
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    format_date(days)
}

/// Howard Hinnant's civil-from-days: pure arithmetic, no date crate needed.
fn format_date(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

/// Builds the `CF_HDROP` payload: a `DROPFILES` header followed by the
/// double-null-terminated UTF-16 path list, as one contiguous byte buffer.
/// One buffer for all paths, whatever the number of ids in the drag.
fn build_cf_hdrop(paths: &[PathBuf]) -> AppResult<Vec<u8>> {
    let header_size = size_of::<DROPFILES>();
    let mut wide_units: Vec<u16> = Vec::new();
    for path in paths {
        wide_units.extend(path.as_os_str().encode_wide());
        wide_units.push(0); // per-path null terminator
    }
    wide_units.push(0); // final null closes the double-null-terminated list

    let mut buf = Vec::with_capacity(header_size + wide_units.len() * 2);
    // DROPFILES, written field by field so no unsafe is needed:
    buf.extend_from_slice(&(header_size as u32).to_le_bytes()); // pFiles: offset of the path list
    buf.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    buf.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    buf.extend_from_slice(&0u32.to_le_bytes()); // fNC: false
    buf.extend_from_slice(&1u32.to_le_bytes()); // fWide: true, paths are UTF-16
    for unit in wide_units {
        buf.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(buf)
}

/// The `IDataObject` handed to `DoDragDrop`. Serves `CF_HDROP` from the
/// prebuilt buffer; every `GetData` returns a fresh `HGLOBAL` that the drop
/// target releases with `ReleaseStgMedium`.
#[implement(IDataObject)]
struct DragDataObject {
    buffer: Vec<u8>,
    formatetc: FORMATETC,
}

impl IDataObject_Impl for DragDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        unsafe {
            // Sound: pformatetcin points to a FORMATETC owned by the drop target and valid for the duration of the call.
            let fmt = *pformatetcin;
            if fmt.cfFormat != CF_HDROP.0 || fmt.tymed & TYMED_HGLOBAL.0 as u32 == 0 {
                return Err(DV_E_FORMATETC.into());
            }
        }
        let size = self.buffer.len();
        // A fresh global block per call: the caller frees it, so sharing one
        // block across calls would hand out freed memory.
        // Sound: GMEM_MOVEABLE with a non-zero size; the handle is freed by the
        // caller, and on every error path below before returning.
        let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, size)? };
        unsafe {
            // Sound: GlobalLock returns a pointer valid for exactly `size` bytes (the allocation above) until GlobalUnlock.
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                // Sound: GlobalFree returns the allocation we own and must not leak on the error path.
                let _ = GlobalFree(Some(hglobal));
                return Err(E_OUTOFMEMORY.into());
            }
            std::ptr::copy_nonoverlapping(self.buffer.as_ptr(), ptr as *mut u8, size);
            // Sound: GlobalUnlock releases the lock on the block we locked, so ReleaseStgMedium can free it.
            let _ = GlobalUnlock(hglobal);
        }
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: hglobal },
            pUnkForRelease: core::mem::ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        unsafe {
            // Sound: pformatetc is a caller-owned FORMATETC valid for the call.
            let fmt = *pformatetc;
            if fmt.cfFormat == CF_HDROP.0 && fmt.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
                HRESULT(0) // S_OK
            } else {
                DV_E_FORMATETC
            }
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        // E_NOTIMPL is the documented "use the original FORMATETC" answer.
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        Ok(IEnumFORMATETC::from(FormatEnum {
            items: vec![self.formatetc],
            pos: Cell::new(0),
        }))
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: windows::core::Ref<'_, IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

/// The single-format enumerator behind `EnumFormatEtc`. `pos` needs interior
/// mutability because the interface's methods only get `&self`.
#[implement(IEnumFORMATETC)]
struct FormatEnum {
    items: Vec<FORMATETC>,
    pos: Cell<usize>,
}

impl IEnumFORMATETC_Impl for FormatEnum_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        if rgelt.is_null() {
            return E_POINTER;
        }
        let remaining = self.items.len().saturating_sub(self.pos.get());
        let count = (celt as usize).min(remaining);
        if count > 0 {
            unsafe {
                // Sound: the caller's contract guarantees rgelt has room for celt FORMATETC elements, and we write at most celt of them.
                std::ptr::copy_nonoverlapping(self.items.as_ptr().add(self.pos.get()), rgelt, count);
            }
        }
        self.pos.set(self.pos.get() + count);
        unsafe {
            // Sound: pceltfetched may be null only when celt == 1; otherwise the COM contract requires it to be writable.
            if !pceltfetched.is_null() {
                *pceltfetched = count as u32;
            }
        }
        if count < celt as usize {
            S_FALSE
        } else {
            HRESULT(0) // S_OK
        }
    }

    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        let remaining = self.items.len().saturating_sub(self.pos.get());
        let skipped = (celt as usize).min(remaining);
        self.pos.set(self.pos.get() + skipped);
        if skipped < celt as usize {
            Err(S_FALSE.into())
        } else {
            Ok(())
        }
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.pos.set(0);
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumFORMATETC> {
        Ok(IEnumFORMATETC::from(FormatEnum {
            items: self.items.clone(),
            pos: Cell::new(self.pos.get()),
        }))
    }
}

/// The `IDropSource` behind the drag: escape cancels, releasing the left
/// button drops, anything else keeps dragging.
#[implement(IDropSource)]
struct DragDropSource;

impl IDropSource_Impl for DragDropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if !grfkeystate.contains(MK_LBUTTON) {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0) // S_OK, keep dragging
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Phase 5. Resolves the ids to real files, then runs the OLE drag. The drag
/// itself happens on a dedicated STA thread because `DoDragDrop` blocks until
/// the user releases the button — on the Tauri command thread that would
/// freeze the UI for the whole drag.
pub fn begin_drag(store: &Store, ids: &[i64]) -> AppResult<()> {
    let paths = resolve_paths(store, ids)?;
    // Only plain data crosses the thread boundary. A COM interface pointer
    // belongs to the apartment that created it and is not valid in another
    // without marshalling, so both objects are built inside the drag thread
    // after it initializes COM. FORMATETC carries a raw pointer and is not
    // Send either, which is the same reason.
    let buffer = build_cf_hdrop(&paths)?;
    std::thread::Builder::new()
        .name("rebuffer-ole-drag".into())
        .spawn(move || run_drag(buffer))
        .map_err(|e| AppError::Other(format!("could not start the drag thread: {e}")))?;
    Ok(())
}

fn run_drag(buffer: Vec<u8>) {
    let _ole = match OleScope::init() {
        Ok(ole) => ole,
        Err(e) => {
            tracing::error!("drag: {e}");
            return;
        }
    };

    // Built here, inside the initialized apartment, for the reason above.
    let formatetc = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let data_object: IDataObject = DragDataObject { buffer, formatetc }.into();
    let drop_source: IDropSource = DragDropSource.into();
    let mut effect = DROPEFFECT_NONE;
    unsafe {
        // Sound: DoDragDrop blocks and pumps messages until the drag ends; both COM objects are owned by this thread and stay alive for the whole call, and pdweffect points at a writable DROPEFFECT. Only COPY is offered so a target can never move (delete) a blob or a referenced original.
        let hr = DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect);
        if hr.is_err() {
            tracing::warn!("drag ended with an error: {hr}");
        }
    }
    tracing::info!("drag finished, effect: {}", effect.0);
    // Release the COM objects while COM is still initialized on this thread.
    drop(data_object);
    drop(drop_source);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    // Only the header-layout test needs POINT, so it lives here rather than in
    // the main imports (which would warn unused in non-test builds).
    use windows::Win32::Foundation::POINT;

    fn item(id: i64, title: Option<&str>, ext: Option<&str>, preview: Option<&str>) -> ItemDto {
        ItemDto {
            id,
            kind: Kind::Other,
            sub_kind: None,
            title: title.map(String::from),
            preview_text: preview.map(String::from),
            thumb_url: None,
            animated_url: None,
            ext: ext.map(String::from),
            byte_size: 0,
            width: None,
            height: None,
            duration_ms: None,
            created_at: 0,
            pinned: false,
            is_reference: false,
            ref_path: None,
            source_app: None,
            copy_count: 1,
            missing: false,
            file_names: Vec::new(),
        }
    }

    #[test]
    fn test_cf_hdrop_single_path() {
        let path = PathBuf::from(r"C:\Users\me\Pictures\shot.png");
        let buf = build_cf_hdrop(&[path.clone()]).unwrap();

        // Header: pFiles == 20, fNC == 0, fWide == 1.
        assert_eq!(u32::from_le_bytes(buf[0..4].try_into().unwrap()), 20);
        assert_eq!(i32::from_le_bytes(buf[4..8].try_into().unwrap()), 0);
        assert_eq!(i32::from_le_bytes(buf[8..12].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(buf[12..16].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(buf[16..20].try_into().unwrap()), 1);

        // Wide section: the path, one null, then one extra null (double null
        // termination of the list).
        let expected_units: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        assert_eq!(buf.len(), 20 + expected_units.len() * 2);
        for (i, unit) in expected_units.iter().enumerate() {
            let off = 20 + i * 2;
            assert_eq!(
                u16::from_le_bytes(buf[off..off + 2].try_into().unwrap()),
                *unit
            );
        }
        // The last two u16s are both zero.
        let tail = &buf[buf.len() - 4..];
        assert_eq!(tail, &[0u8, 0, 0, 0]);
    }

    #[test]
    fn test_cf_hdrop_three_paths() {
        let paths = [
            PathBuf::from(r"C:\a.txt"),
            PathBuf::from(r"D:\b\b.txt"),
            PathBuf::from(r"E:\c.png"),
        ];
        let buf = build_cf_hdrop(&paths).unwrap();

        // The whole wide section: each path null-terminated, plus a final null.
        let mut expected: Vec<u16> = Vec::new();
        for p in &paths {
            expected.extend(p.as_os_str().encode_wide());
            expected.push(0);
        }
        expected.push(0);
        assert_eq!(buf.len(), 20 + expected.len() * 2);
        for (i, unit) in expected.iter().enumerate() {
            let off = 20 + i * 2;
            assert_eq!(
                u16::from_le_bytes(buf[off..off + 2].try_into().unwrap()),
                *unit
            );
        }
        // Path boundaries are where we pushed nulls: every path round-trips.
        let mut off = 20;
        for p in &paths {
            let units = p.as_os_str().encode_wide().count();
            let path_bytes = &buf[off..off + units * 2];
            let decoded: Vec<u16> = path_bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            assert_eq!(String::from_utf16_lossy(&decoded), p.to_string_lossy());
            off += units * 2 + 2; // + the per-path null
        }
    }

    #[test]
    fn test_cf_hdrop_unicode_path() {
        let path = PathBuf::from(r"C:\tmp\фото.png");
        let buf = build_cf_hdrop(&[path.clone()]).unwrap();
        let units: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        assert_eq!(buf.len(), 20 + units.len() * 2);
        for (i, unit) in units.iter().enumerate() {
            let off = 20 + i * 2;
            assert_eq!(
                u16::from_le_bytes(buf[off..off + 2].try_into().unwrap()),
                *unit
            );
        }
    }

    #[test]
    fn test_cf_hdrop_header_matches_dropfiles_layout() {
        assert_eq!(size_of::<DROPFILES>(), 20);
        let buf = build_cf_hdrop(&[PathBuf::from(r"C:\x.txt")]).unwrap();
        let header = DROPFILES {
            pFiles: 20,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        let mut expected = [0u8; 20];
        unsafe {
            // Sound: DROPFILES is repr(C, packed(1)) with no padding, so copying its 20 bytes reproduces the exact header layout.
            std::ptr::copy_nonoverlapping(
                &header as *const DROPFILES as *const u8,
                expected.as_mut_ptr(),
                20,
            );
        }
        assert_eq!(&buf[..20], &expected);
    }

    #[test]
    fn test_temp_name_prefers_title() {
        let it = item(1, Some("Quarterly Report"), Some("PDF"), None);
        assert_eq!(temp_file_name_on(&it, "2026-08-30"), "Quarterly Report.pdf");
    }

    #[test]
    fn test_temp_name_sanitizes_illegal_title_chars() {
        let it = item(1, Some("Q3: report* (final)?"), Some("TXT"), None);
        assert_eq!(temp_file_name_on(&it, "2026-08-30"), "Q3_ report_ (final)_.txt");
    }

    #[test]
    fn test_temp_name_derived_from_preview() {
        let it = item(1, None, Some("TXT"), Some("hello world\nsecond line"));
        assert_eq!(temp_file_name_on(&it, "2026-08-30"), "hello world.txt");
    }

    #[test]
    fn test_temp_name_no_title_no_ext() {
        let it = item(1, None, None, None);
        assert_eq!(temp_file_name_on(&it, "2026-08-30"), "rebuffer-2026-08-30");
    }

    #[test]
    fn test_temp_name_image_fallback() {
        let mut it = item(1, None, Some("PNG"), None);
        it.kind = Kind::Image;
        assert_eq!(temp_file_name_on(&it, "2026-08-30"), "screenshot-2026-08-30.png");
    }

    #[test]
    fn test_sanitize_reserved_names_and_trailing_dots() {
        assert_eq!(sanitize_stem("CON"), "_CON");
        assert_eq!(sanitize_stem("nul"), "_nul");
        assert_eq!(sanitize_stem("notes."), "notes");
        assert_eq!(sanitize_stem("notes...   "), "notes");
        assert_eq!(sanitize_stem("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_stem(""), "");
        assert_eq!(sanitize_stem("..."), "");
    }

    #[test]
    fn test_suffixed_keeps_extension() {
        assert_eq!(suffixed("foo.png", 1), "foo-1.png");
        assert_eq!(suffixed("foo", 1), "foo-1");
        assert_eq!(suffixed("no.ext", 7), "no-7.ext");
    }

    #[test]
    fn test_format_date() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(20_695), "2026-08-30");
        assert_eq!(format_date(365), "1971-01-01");
        // 1970-01-01 + 19448 days is 2023-04-01; the previous expectation
        // here was simply miscounted.
        assert_eq!(format_date(19_448), "2023-04-01");
    }

    #[test]
    fn test_materialize_reuses_same_item() {
        let dir = tempdir().unwrap();
        let blob = dir.path().join("blob-src");
        std::fs::write(&blob, b"payload").unwrap();
        let mut cache = HashMap::new();
        let it = item(7, Some("same"), Some("PNG"), None);

        let first = materialize_into(dir.path(), &mut cache, &blob, &it).unwrap();
        assert!(first.exists());
        assert_eq!(std::fs::read(&first).unwrap(), b"payload");
        assert_eq!(first.file_name().unwrap(), "same.png");

        let second = materialize_into(dir.path(), &mut cache, &blob, &it).unwrap();
        assert_eq!(second, first, "same item must reuse its materialized file");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_materialize_suffixes_name_collisions() {
        let dir = tempdir().unwrap();
        let blob_a = dir.path().join("blob-a");
        let blob_b = dir.path().join("blob-b");
        std::fs::write(&blob_a, b"aaa").unwrap();
        std::fs::write(&blob_b, b"bbb").unwrap();
        let mut cache = HashMap::new();

        let a = materialize_into(dir.path(), &mut cache, &blob_a, &item(1, Some("same"), Some("png"), None)).unwrap();
        let b = materialize_into(dir.path(), &mut cache, &blob_b, &item(2, Some("same"), Some("png"), None)).unwrap();
        assert_eq!(a.file_name().unwrap(), "same.png");
        assert_eq!(b.file_name().unwrap(), "same-1.png");
        assert_eq!(std::fs::read(&b).unwrap(), b"bbb");

        let c = materialize_into(dir.path(), &mut cache, &blob_b, &item(3, None, None, None)).unwrap();
        assert!(c.file_name().unwrap().to_string_lossy().starts_with("rebuffer-"));
    }
}