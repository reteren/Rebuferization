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
    AddClipboardFormatListener, GetClipboardSequenceNumber, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW, TranslateMessage,
    UnregisterClassW, CREATESTRUCTW, GWLP_USERDATA, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_CREATE, WM_DESTROY, WM_NCDESTROY, WNDCLASSEXW,
    HWND_MESSAGE,
};

use crate::clipboard::decode;
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

            // 4. Open clipboard with retry (10 attempts, 20ms backoff)
            let guard = match writer::ClipboardGuard::open_with_retry(Some(hwnd)) {
                Ok(g) => g,
                Err(_) => {
                    // Give up silently if another app holds the clipboard
                    return LRESULT(0);
                }
            };

            // 5. Privacy filter BEFORE decoding
            let privacy_settings = ctx.load_privacy_settings();
            if privacy::check_clipboard_privacy(source_app.as_deref(), &privacy_settings) {
                return LRESULT(0);
            }

            // 6. Decode clipboard content
            let max_bytes = ctx.load_max_item_bytes();
            let capture_opt = decode::decode_clipboard(max_bytes, source_app);

            // 7. Explicitly release clipboard before inserting into store
            drop(guard);

            // 8. Synchronously insert into Store and call on_item callback
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
