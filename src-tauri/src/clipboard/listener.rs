//! Dedicated message-only window thread listening for `WM_CLIPBOARDUPDATE`.
//!
//! OWNER: worker W2.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::thread;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardOwner, GetClipboardSequenceNumber,
    RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, GetWindowThreadProcessId, PostQuitMessage, RegisterClassExW,
    SendMessageTimeoutW, SetWindowLongPtrW, TranslateMessage, UnregisterClassW, CREATESTRUCTW,
    GWLP_USERDATA, HWND_MESSAGE, MSG, SMTO_ABORTIFHUNG, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WM_CREATE, WM_DESTROY, WM_NCDESTROY, WM_NULL, WNDCLASSEXW,
};

use crate::clipboard::decode;

/// How long to wait after `WM_CLIPBOARDUPDATE` before looking at the clipboard
/// at all. Short; the real waiting is done by `owner_is_writing`.
const OPEN_DELAY: std::time::Duration = std::time::Duration::from_millis(10);

/// How long to give the clipboard's owner to answer a ping before concluding it
/// is still busy writing.
const OWNER_PING_MS: u32 = 40;

/// How long to wait between pings, and how many times to try, before giving up
/// on a capture entirely. Roughly half a second in total: longer than any
/// normal write takes, short enough that a genuinely stuck owner cannot defer
/// captures for ever.
const SETTLE_STEP: std::time::Duration = std::time::Duration::from_millis(60);
const SETTLE_ATTEMPTS: u32 = 8;

/// Whether the app that owns the clipboard is still in the middle of writing to
/// it.
///
/// This is the difference between a clipboard manager and a broken machine.
/// A write like WPF's `SetDataObject(data, copy: true)` — which is what a
/// screenshot tool does — happens in two steps: the formats are published for
/// *delayed* rendering, and then flushed so they are actually materialised.
/// Between the two the clipboard is briefly free.
///
/// A reader that takes it in that gap then asks for a format nobody has
/// rendered yet, and Windows answers by sending `WM_RENDERFORMAT` to the
/// owner — which is inside its own write, waiting for the clipboard this reader
/// is holding. Neither side can move. Measured here: 995 ms of held clipboard,
/// and the writer's flush failing outright with `CLIPBRD_E_CANT_OPEN`, which is
/// how this application broke a screenshot tool that had done nothing wrong.
///
/// An owner that is mid-write is not pumping messages, so a ping that goes
/// unanswered is exactly the signal to stay out of the way. Measured: no answer
/// within 40 ms while the write was in flight, an answer in 0.1 ms once it had
/// finished.
unsafe fn owner_is_writing() -> bool {
    // Sound: GetClipboardOwner is a read-only query needing no open clipboard.
    let owner = GetClipboardOwner().unwrap_or_default();
    if owner.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    // Sound: owner is a live window handle; pid is a plain out-param.
    GetWindowThreadProcessId(owner, Some(&mut pid));
    // Our own writes are already filtered by the sequence number, and pinging
    // ourselves from this thread would be answered by this thread.
    if pid == GetCurrentProcessId() {
        return false;
    }
    let mut result = 0usize;
    // Sound: WM_NULL carries no data and is safe to send to any window; the
    // timeout bounds the wait, and ABORTIFHUNG returns early for a dead one.
    let answered = SendMessageTimeoutW(
        owner,
        WM_NULL,
        WPARAM(0),
        LPARAM(0),
        SMTO_ABORTIFHUNG,
        OWNER_PING_MS,
        Some(&mut result),
    );
    answered.0 == 0
}
use crate::clipboard::privacy;
use crate::clipboard::writer;
use crate::clipboard::ClipboardWatcher;
use crate::clipboard::OnItem;
use crate::error::{AppError, AppResult};
use crate::settings::{PrivacySettings, Settings};
use crate::store::Store;

pub struct ListenerContext {
    pub store: Arc<Store>,
    pub on_item: OnItem,
    pub enabled: Arc<AtomicBool>,
}

impl ListenerContext {
    pub fn load_privacy_settings(&self) -> PrivacySettings {
        let settings_path = self.store.root().join("settings.json");
        if settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                if let Ok(s) = serde_json::from_str::<Settings>(&content) {
                    return s.privacy;
                }
            }
        }
        PrivacySettings::default()
    }

    pub fn load_max_item_bytes(&self) -> u64 {
        let settings_path = self.store.root().join("settings.json");
        if settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                if let Ok(s) = serde_json::from_str::<Settings>(&content) {
                    return s.storage.max_item_bytes;
                }
            }
        }
        256 * 1024 * 1024 // 256 MB default
    }
}

/// Spawns the dedicated listener thread and blocks until the hidden window is created.
pub fn spawn_listener(store: Arc<Store>, on_item: OnItem) -> AppResult<ClipboardWatcher> {
    let (tx, rx) = sync_channel::<AppResult<isize>>(1);
    let enabled = Arc::new(AtomicBool::new(true));
    let enabled_clone = enabled.clone();
    let store_clone = store.clone();

    let join_handle = thread::Builder::new()
        .name("clipboard-listener".into())
        .spawn(move || {
            run_listener_thread(store_clone, on_item, enabled_clone, tx);
        })
        .map_err(|e| AppError::Other(format!("Failed to spawn listener thread: {e}")))?;

    let hwnd_raw = rx
        .recv()
        .map_err(|_| AppError::Other("Listener thread exited prematurely".into()))??;

    Ok(ClipboardWatcher {
        store,
        enabled,
        hwnd_raw,
        join_handle: parking_lot::Mutex::new(Some(join_handle)),
    })
}

fn run_listener_thread(
    store: Arc<Store>,
    on_item: OnItem,
    enabled: Arc<AtomicBool>,
    tx: std::sync::mpsc::SyncSender<AppResult<isize>>,
) {
    let class_name = w!("RebufferClipboardListenerClass");

    unsafe {
        // Sound: GetModuleHandleW(None) retrieves HINSTANCE of current executable module safely without mutation.
        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h.into(),
            Err(e) => {
                let _ = tx.send(Err(AppError::Win(e.to_string())));
                return;
            }
        };

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(listener_wndproc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };

        // Sound: RegisterClassExW registers our message-only window class with valid struct and function pointer.
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            let _ = tx.send(Err(AppError::Other("RegisterClassExW failed".into())));
            return;
        }

        let context = Box::new(ListenerContext {
            store,
            on_item,
            enabled,
        });
        let context_ptr = Box::into_raw(context);

        // Sound: CreateWindowExW creates a hidden message-only window parented to HWND_MESSAGE; passes raw heap pointer as creation param.
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("RebufferClipboardListener"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            Some(context_ptr as *const core::ffi::c_void),
        ) {
            Ok(h) if !h.0.is_null() => h,
            _ => {
                let _ = Box::from_raw(context_ptr);
                let _ = UnregisterClassW(class_name, Some(hinstance));
                let _ = tx.send(Err(AppError::Other("CreateWindowExW failed".into())));
                return;
            }
        };

        // Sound: AddClipboardFormatListener registers valid window handle hwnd to receive WM_CLIPBOARDUPDATE messages.
        if let Err(e) = AddClipboardFormatListener(hwnd) {
            let _ = DestroyWindow(hwnd);
            let _ = UnregisterClassW(class_name, Some(hinstance));
            let _ = tx.send(Err(AppError::Win(e.to_string())));
            return;
        }

        // Notify parent thread that window exists and listener is ready
        let _ = tx.send(Ok(hwnd.0 as isize));

        // Sound: msg is stack-allocated; GetMessage blocks with 0% idle CPU until window messages arrive.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Sound: Unregisters window class using module HINSTANCE after message loop terminates.
        let _ = UnregisterClassW(class_name, Some(hinstance));
    }
}

unsafe extern "system" fn listener_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            // Sound: lparam points to valid CREATESTRUCTW delivered by Windows during CreateWindowExW.
            if lparam.0 != 0 {
                let cs = &*(lparam.0 as *const CREATESTRUCTW);
                if !cs.lpCreateParams.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
                }
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            // Sound: Retrieves raw pointer to ListenerContext stored during WM_CREATE, which remains valid until WM_NCDESTROY.
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ListenerContext;
            if ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let ctx = &*ptr;

            // 1. If capture disabled, skip
            if !ctx.enabled.load(Ordering::SeqCst) {
                return LRESULT(0);
            }

            // 2. Ignore writes originating from our own app
            // Sound: GetClipboardSequenceNumber is a thread-safe Win32 query without preconditions.
            let seq = GetClipboardSequenceNumber();
            if writer::is_our_sequence(seq) {
                return LRESULT(0);
            }

            // 3. Detect foreground app BEFORE opening clipboard
            let source_app = privacy::get_foreground_process_name();

            // 4. Privacy filter before the clipboard is opened at all. It needs
            //    only the foreground process and the settings, both already in
            //    hand, and a blocked app must not cost every other process on
            //    the machine an open/close cycle of the global lock.
            let privacy_settings = ctx.load_privacy_settings();
            if privacy::check_clipboard_privacy(source_app.as_deref(), &privacy_settings) {
                return LRESULT(0);
            }

            // 5. Let the writer finish before reaching for the lock.
            //
            //    `WM_CLIPBOARDUPDATE` arrives while the app that caused it may
            //    still be halfway through its write, and taking the clipboard
            //    there is what deadlocks the two of us; see `owner_is_writing`.
            //    Waiting costs a capture nothing — it is already asynchronous
            //    from the user's point of view.
            std::thread::sleep(OPEN_DELAY);
            let mut settled = false;
            for _ in 0..SETTLE_ATTEMPTS {
                if !owner_is_writing() {
                    settled = true;
                    break;
                }
                std::thread::sleep(SETTLE_STEP);
            }
            if !settled {
                // Better to lose one history entry than to hold the clipboard
                // against an app that is still trying to use it.
                tracing::debug!("clipboard owner still busy; skipping this capture");
                return LRESULT(0);
            }

            // 6. Open clipboard with retry (10 attempts, 20ms backoff)
            let guard = match writer::ClipboardGuard::open_with_retry(Some(hwnd)) {
                Ok(g) => g,
                Err(_) => {
                    // Give up silently if another app holds the clipboard
                    return LRESULT(0);
                }
            };

            // 7. Copy the contents out — and nothing more. Every millisecond
            //    between here and the `drop` below is a millisecond in which no
            //    other process on the machine can write to the clipboard at
            //    all; see `decode::RawClipboard` for what that cost.
            let max_bytes = ctx.load_max_item_bytes();
            let raw = decode::grab_clipboard(max_bytes);

            // 8. Release the clipboard before doing any work with what was read.
            drop(guard);

            // 9. Decode with the lock released: a PNG parsed for its
            //    dimensions, a DIB turned into a PNG, a stat per dropped file.
            let capture_opt = match raw {
                Ok(Some(raw)) => decode::build_capture(raw, max_bytes, source_app),
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            };

            // 10. Synchronously insert into Store and call on_item callback
            if let Ok(Some(capture)) = capture_opt {
                match ctx.store.insert_capture(capture) {
                    Ok(item_dto) => {
                        (ctx.on_item)(item_dto);
                    }
                    Err(e) => {
                        tracing::error!("Failed to insert capture into store: {e}");
                    }
                }
            }

            LRESULT(0)
        }
        WM_DESTROY => {
            // Sound: Unregisters clipboard format listener for hwnd and posts WM_QUIT to terminate the GetMessage loop.
            let _ = RemoveClipboardFormatListener(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // Sound: Reclaims heap allocation for ListenerContext originally created by Box::into_raw during CreateWindowExW.
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut ListenerContext;
            if !ptr.is_null() {
                let _ = Box::from_raw(ptr);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
