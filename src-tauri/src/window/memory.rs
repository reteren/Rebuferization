//! Working-set trimming for the idle tray app.
//!
//! OWNER: worker W3.
//!
//! A clipboard app spends almost all of its life with nothing on screen, and a
//! WebView2 host is a small tree of processes rather than one: the browser
//! process, the GPU process, the network and storage utilities, the crashpad
//! handler and a renderer per page. Every one of them keeps the pages it has
//! touched resident long after the popup has gone.
//!
//! `EmptyWorkingSet` asks Windows to unmap those pages from the process's
//! working set. It is honest about what it does not do: the commit charge is
//! unchanged, and the pages land on the standby list rather than being freed,
//! so the next show faults them back in — soft faults from RAM, not disk reads,
//! unless the machine was under memory pressure in the meantime. What it buys
//! is that a tray app sitting idle stops holding ~200 MB of *resident* memory
//! that the rest of the system could be using.
//!
//! Everything here is best effort. A failure to snapshot, open or trim a
//! process is a debug line and nothing more — trimming is an optimisation, and
//! an optimisation must never be able to take the app down.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SET_QUOTA,
};

use super::{POPUP_LABEL, SETTINGS_LABEL, TRAYMENU_LABEL};

/// The only descendants that are ever trimmed. The WebView2 tree is entirely
/// `msedgewebview2.exe`, and a name check keeps a recycled PID — or a program
/// the user opened from the app through the shell — from being trimmed on the
/// strength of a parent id alone.
const WEBVIEW_PROCESS: &str = "msedgewebview2.exe";

/// How long a scheduled trim waits before running, and the guard that keeps a
/// burst of hides from spawning a thread each.
static TRIM_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// The interval of the background pass. Long on purpose: the pass exists to
/// catch the idle hours, not to chase individual interactions.
const IDLE_PASS_INTERVAL: Duration = Duration::from_secs(300);

/// Trims this process and every WebView2 process it owns, unless something of
/// ours is on screen. Never returns an error and never panics.
pub fn trim_idle(app: &AppHandle) {
    if anything_visible(app) {
        tracing::debug!("trim skipped: a window is visible");
        return;
    }
    trim_self();
    let pids = webview_descendants();
    let count = pids.len();
    for pid in pids {
        trim_pid(pid);
    }
    tracing::debug!("working set trimmed (self + {count} webview processes)");
}

/// Runs `trim_idle` on a background thread after `delay`. A second request
/// while one is already pending is dropped: rapid open/close cycles must not
/// spawn a thread per hide, and the pending pass will see the final state
/// anyway.
pub fn schedule_trim(app: &AppHandle, delay: Duration) {
    if TRIM_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        TRIM_SCHEDULED.store(false, Ordering::SeqCst);
        trim_idle(&app);
    });
}

/// Starts the low-frequency background pass. It no-ops while anything is
/// visible, so it costs a process snapshot every five minutes and nothing else.
pub fn start_idle_pass(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(IDLE_PASS_INTERVAL);
        trim_idle(&app);
    });
}

/// True when any window of ours is on screen. A window that is absent (the
/// lazily built ones spend most of their life that way) is not visible.
fn anything_visible(app: &AppHandle) -> bool {
    [POPUP_LABEL, SETTINGS_LABEL, TRAYMENU_LABEL]
        .iter()
        .any(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false)
        })
}

fn trim_self() {
    // FFI: GetCurrentProcess returns a pseudo-handle that needs no closing.
    if let Err(e) = unsafe { EmptyWorkingSet(GetCurrentProcess()) } {
        tracing::debug!("EmptyWorkingSet(self) failed: {e}");
    }
}

fn trim_pid(pid: u32) {
    // FFI: a plain open/use/close of one process handle. Both rights are
    // mandatory, not one plus a convenience: EmptyWorkingSet is documented as
    // needing PROCESS_SET_QUOTA *and* either PROCESS_QUERY_INFORMATION or
    // PROCESS_QUERY_LIMITED_INFORMATION. Dropping the query right would make
    // every call fail with access denied.
    let handle: HANDLE = match unsafe {
        OpenProcess(
            PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    } {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!("OpenProcess({pid}) failed: {e}");
            return;
        }
    };
    if let Err(e) = unsafe { EmptyWorkingSet(handle) } {
        tracing::debug!("EmptyWorkingSet({pid}) failed: {e}");
    }
    let _ = unsafe { CloseHandle(handle) };
}

/// The PIDs of every `msedgewebview2.exe` in our process tree.
///
/// One snapshot is taken and the whole tree is walked inside it: WebView2's
/// browser process is our direct child, but the renderers, the GPU process and
/// the utilities are *its* children, so a single parent-id check would trim the
/// browser process alone and miss the ~150 MB below it.
fn webview_descendants() -> Vec<u32> {
    let Some(entries) = process_table() else {
        return Vec::new();
    };

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut names: HashMap<u32, String> = HashMap::new();
    for (pid, parent, name) in entries {
        // A process listing its own PID as its parent would make the walk
        // below run forever.
        if pid != parent {
            children.entry(parent).or_default().push(pid);
        }
        names.insert(pid, name);
    }

    // FFI: no arguments, cannot fail.
    let own = unsafe { GetCurrentProcessId() };
    let mut seen: HashSet<u32> = HashSet::from([own]);
    let mut queue = vec![own];
    let mut out = Vec::new();
    while let Some(pid) = queue.pop() {
        for &child in children.get(&pid).map(Vec::as_slice).unwrap_or_default() {
            if !seen.insert(child) {
                continue;
            }
            queue.push(child);
            if names
                .get(&child)
                .is_some_and(|n| n.eq_ignore_ascii_case(WEBVIEW_PROCESS))
            {
                out.push(child);
            }
        }
    }
    out
}

/// `(pid, parent pid, image name)` for every process on the machine, or `None`
/// if the snapshot could not be taken.
fn process_table() -> Option<Vec<(u32, u32, String)>> {
    // FFI: the snapshot handle is closed on every path out of this function.
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!("CreateToolhelp32Snapshot failed: {e}");
            return None;
        }
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut out = Vec::new();
    // FFI: `entry` is a correctly sized stack buffer; the iteration ends when
    // Process32NextW reports there are no more entries.
    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                out.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    exe_name(&entry.szExeFile),
                ));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    Some(out)
}

/// The NUL-terminated image name from a `PROCESSENTRY32W`.
fn exe_name(raw: &[u16; 260]) -> String {
    let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_stops_at_the_terminator() {
        let mut raw = [0u16; 260];
        for (slot, c) in raw.iter_mut().zip("edge.exe".encode_utf16()) {
            *slot = c;
        }
        assert_eq!(exe_name(&raw), "edge.exe");
    }

    /// The walk has to reach our own process and no further up, and it must
    /// terminate on a tree that contains a cycle.
    #[test]
    fn the_descendant_walk_includes_the_webview_tree() {
        // Our own process always exists, so this exercises the real snapshot
        // path; the app under test has no webview children, so the only
        // guarantee is that it terminates and never lists us.
        let own = unsafe { GetCurrentProcessId() };
        assert!(!webview_descendants().contains(&own));
    }
}
