//! Privacy filtering for clipboard captures.
//!
//! Evaluated BEFORE decoding clipboard content to prevent password managers
//! and excluded applications from polluting the clipboard database.
//!
//! OWNER: worker W2.

use std::path::Path;

use once_cell::sync::Lazy;
use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    GetClipboardData, IsClipboardFormatAvailable, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::settings::PrivacySettings;

/// Registered format: "ExcludeClipboardContentFromMonitorProcessing".
/// When present on clipboard, monitor MUST skip capture.
pub static FORMAT_EXCLUDE_MONITOR: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and lives for the entire program execution.
    RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
});

/// Registered format: "CanIncludeInClipboardHistory".
/// When present with DWORD 0, history MUST skip capture.
pub static FORMAT_CAN_INCLUDE_HISTORY: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and lives for the entire program execution.
    RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"))
});

/// Registered format: "CanUploadToCloudClipboard".
/// When present with DWORD 0, cloud upload is disabled (logged but non-blocking).
pub static FORMAT_CAN_UPLOAD_CLOUD: Lazy<u32> = Lazy::new(|| unsafe {
    // Sound: RegisterClipboardFormatW takes a static wide string literal that is null-terminated and lives for the entire program execution.
    RegisterClipboardFormatW(w!("CanUploadToCloudClipboard"))
});

/// Privacy flags extracted from the active clipboard content.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardPrivacyFlags {
    pub exclude_from_monitor: bool,
    pub can_include_in_history: Option<u32>,
    pub can_upload_to_cloud: Option<u32>,
}

/// Pure decision function: returns `true` if the capture should be skipped.
pub fn should_skip(
    flags: &ClipboardPrivacyFlags,
    foreground_exe: Option<&str>,
    settings: &PrivacySettings,
) -> bool {
    // 1. Process blocklist check (case-insensitive on executable name)
    if let Some(exe) = foreground_exe {
        let exe_lower = exe.to_ascii_lowercase();
        if settings
            .blocked_processes
            .iter()
            .any(|blocked| blocked.to_ascii_lowercase() == exe_lower)
        {
            return true;
        }
    }

    // 2. Clipboard privacy flags check (if respect_clipboard_flags is true)
    if settings.respect_clipboard_flags {
        if flags.exclude_from_monitor {
            return true;
        }
        if let Some(val) = flags.can_include_in_history {
            if val == 0 {
                return true;
            }
        }
    }

    false
}

/// Checks if a clipboard format is currently available on the system clipboard.
pub fn is_format_present(format: u32) -> bool {
    if format == 0 {
        return false;
    }
    unsafe {
        // Sound: IsClipboardFormatAvailable performs a read-only query on registered or system format ID without mutating state.
        IsClipboardFormatAvailable(format).is_ok()
    }
}

/// Reads a DWORD (u32) value for a given clipboard format.
/// Assumes the clipboard is already opened on the current thread.
pub fn read_clipboard_dword(format: u32) -> Option<u32> {
    if format == 0 {
        return None;
    }
    unsafe {
        // Sound: IsClipboardFormatAvailable checks format presence while clipboard is open on this thread.
        if !IsClipboardFormatAvailable(format).is_ok() {
            return None;
        }
        // Sound: GetClipboardData retrieves the global handle owned by Windows for format on open clipboard.
        let handle = match GetClipboardData(format) {
            Ok(h) if !h.0.is_null() => h,
            _ => return None,
        };
        let hglobal = HGLOBAL(handle.0);
        // Sound: GlobalSize queries the byte length of the HGLOBAL allocation.
        let size = GlobalSize(hglobal);
        if size < 4 {
            return None;
        }
        // Sound: GlobalLock obtains a pointer to the clipboard data buffer valid until GlobalUnlock.
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        // Sound: ptr is non-null and size >= 4, so reading 4 bytes unaligned is valid.
        let val = std::ptr::read_unaligned(ptr as *const u32);
        // Sound: GlobalUnlock releases the lock acquired by GlobalLock on hglobal.
        let _ = GlobalUnlock(hglobal);
        Some(val)
    }
}

/// Retrieves the executable file name of the currently active foreground window.
pub fn get_foreground_process_name() -> Option<String> {
    unsafe {
        // Sound: GetForegroundWindow returns the top-level foreground HWND or null safely without side effects.
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        // Sound: Passing valid stack reference to receive the 32-bit process ID for hwnd.
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        // Sound: OpenProcess requests minimal PROCESS_QUERY_LIMITED_INFORMATION rights; handle is closed below via CloseHandle.
        let process: HANDLE = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(p) => p,
            Err(_) => return None,
        };
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        // Sound: QueryFullProcessImageNameW writes up to size wide chars into the 1024-element stack buffer.
        let success = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        // Sound: CloseHandle frees the process handle to prevent resource leaks.
        let _ = CloseHandle(process);

        if success.is_ok() && size > 0 {
            let actual_size = (size as usize).min(buf.len());
            let full_path = String::from_utf16_lossy(&buf[..actual_size]);
            Path::new(&full_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

/// Inspects the open clipboard for privacy flags and determines if capture should be skipped.
/// Assumes clipboard is open on this thread.
pub fn check_clipboard_privacy(
    foreground_exe: Option<&str>,
    settings: &PrivacySettings,
) -> bool {
    let mut flags = ClipboardPrivacyFlags::default();

    if settings.respect_clipboard_flags {
        flags.exclude_from_monitor = is_format_present(*FORMAT_EXCLUDE_MONITOR);
        flags.can_include_in_history = read_clipboard_dword(*FORMAT_CAN_INCLUDE_HISTORY);
        flags.can_upload_to_cloud = read_clipboard_dword(*FORMAT_CAN_UPLOAD_CLOUD);

        if let Some(cloud_val) = flags.can_upload_to_cloud {
            if cloud_val == 0 {
                tracing::info!("CanUploadToCloudClipboard = 0 (logged, non-blocking)");
            }
        }
    }

    should_skip(&flags, foreground_exe, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_privacy_settings() -> PrivacySettings {
        PrivacySettings {
            respect_clipboard_flags: true,
            blocked_processes: vec![
                "keepass.exe".into(),
                "keepassxc.exe".into(),
                "1password.exe".into(),
                "bitwarden.exe".into(),
                "lastpass.exe".into(),
                "dashlane.exe".into(),
                "protonpass.exe".into(),
            ],
        }
    }

    #[test]
    fn test_process_blocklist_case_insensitive() {
        let settings = default_privacy_settings();
        let flags = ClipboardPrivacyFlags::default();

        assert!(should_skip(&flags, Some("keepass.exe"), &settings));
        assert!(should_skip(&flags, Some("KeePass.EXE"), &settings));
        assert!(should_skip(&flags, Some("Bitwarden.exe"), &settings));
        assert!(should_skip(&flags, Some("1PASSWORD.EXE"), &settings));
        assert!(should_skip(&flags, Some("protonpass.exe"), &settings));

        assert!(!should_skip(&flags, Some("code.exe"), &settings));
        assert!(!should_skip(&flags, Some("notepad.exe"), &settings));
        assert!(!should_skip(&flags, Some("chrome.exe"), &settings));
        assert!(!should_skip(&flags, None, &settings));
    }

    #[test]
    fn test_exclude_monitor_flag() {
        let settings = default_privacy_settings();
        let flags = ClipboardPrivacyFlags {
            exclude_from_monitor: true,
            can_include_in_history: None,
            can_upload_to_cloud: None,
        };

        assert!(should_skip(&flags, Some("notepad.exe"), &settings));

        let flags_disabled = flags.clone();
        let mut settings_no_flags = settings.clone();
        settings_no_flags.respect_clipboard_flags = false;
        assert!(!should_skip(&flags_disabled, Some("notepad.exe"), &settings_no_flags));
    }

    #[test]
    fn test_can_include_in_history_flag() {
        let settings = default_privacy_settings();

        // 0 = skip
        let flags_zero = ClipboardPrivacyFlags {
            exclude_from_monitor: false,
            can_include_in_history: Some(0),
            can_upload_to_cloud: None,
        };
        assert!(should_skip(&flags_zero, Some("notepad.exe"), &settings));

        // 1 = allow
        let flags_one = ClipboardPrivacyFlags {
            exclude_from_monitor: false,
            can_include_in_history: Some(1),
            can_upload_to_cloud: None,
        };
        assert!(!should_skip(&flags_one, Some("notepad.exe"), &settings));
    }

    #[test]
    fn test_can_upload_to_cloud_does_not_block() {
        let settings = default_privacy_settings();
        let flags_cloud_zero = ClipboardPrivacyFlags {
            exclude_from_monitor: false,
            can_include_in_history: Some(1),
            can_upload_to_cloud: Some(0),
        };
        // CanUploadToCloudClipboard = 0 must NOT block
        assert!(!should_skip(&flags_cloud_zero, Some("notepad.exe"), &settings));
    }
}
