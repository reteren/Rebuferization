# Adversarial Review of the Rebuffer Rust Engine

**Review Scope**: `src-tauri/src/store/**` (~2600 lines) and `src-tauri/src/clipboard/**` (~2300 lines).  
**Review Target**: `docs/SPEC.md` §2 and §3 contracts and edge cases.  
**Review Confidence**: High for Win32 resource lifecycle, concurrency/lock discipline, SQLite transaction semantics, and privacy filtering. Manual inspection performed across all public entry points, error paths, and background threads. (Out of scope / unreviewed: Tauri frontend WebView2 JS/Svelte bindings and tray window UI styling).

---

## Executive Summary & Findings Overview

| # | Severity | Category | File & Location | Summary |
|---|---|---|---|---|
| **1** | **Critical** | Privacy Filter | `src/clipboard/privacy.rs:133-174` | Privacy filter fails open when foreground process is elevated (UAC) or inaccessible |
| **2** | **High** | Privacy Filter | `src/clipboard/privacy.rs:59-69` | Blocklist comparison fails when user configures full executable paths via settings |
| **3** | **High** | Lock Discipline | `src/store/mod.rs:124-132, 291-301` | AB-BA Deadlock between `store.switch_root()` and UI store reads (`list`/`search`/`stats`) |
| **4** | **Medium-High** | Dedup & Refcount | `src/store/queries.rs:434-454`, `store/mod.rs:171-195` | Race condition between non-transactional `delete_items` and `insert_capture` creating orphan rows with missing blobs |
| **5** | **Medium** | Dedup & Refcount | `src/store/blobs.rs:167-197` | Derived refcount deletion checks `items` but ignores secondary format blob references in `item_formats` |
| **6** | **Medium** | Win32 State | `src/clipboard/writer.rs:86-92, 220-226` | Sequence number tracking skipped on write errors, causing self-capture loops |
| **7** | **Medium** | Janitor Size Cap | `src/store/mod.rs:94-114` | Hourly background janitor thread hardcodes `max_store_bytes = None`, never running size cap pruning |
| **8** | **Low-Medium** | Lock Discipline | `src/store/mod.rs:168-195` | SQLite connection mutex held across blocking disk `fsync` syscalls in `insert_capture` |

---

## Detailed Findings

#### 1. Privacy filter fails open when foreground process is elevated (UAC) or inaccessible
- **File & Line**: `src-tauri/src/clipboard/privacy.rs:133-174`, `src-tauri/src/clipboard/privacy.rs:59-69`
- **Category**: 3 — The Privacy Filter
- **Severity**: Critical
- **Assignment**: Assigned to clipboard worker.
- **What breaks**: When the foreground window belongs to an elevated process (e.g., KeePassXC or 1Password running as Administrator while Rebuffer runs as standard user), `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, ...)` fails with `ERROR_ACCESS_DENIED`. `get_foreground_process_name()` returns `None`. In `should_skip()`, `if let Some(exe) = foreground_exe` evaluates to `false`, allowing the capture to proceed.
- **Concrete Failure Scenario**:
  1. User runs KeePassXC with elevated privileges (Run as Administrator) or Windows UAC prompt is active.
  2. User copies a master password or private key.
  3. `WM_CLIPBOARDUPDATE` fires. Rebuffer's listener executes `privacy::get_foreground_process_name()`.
  4. `OpenProcess` returns `Err(AccessDenied)`, and `get_foreground_process_name()` returns `None`.
  5. `check_clipboard_privacy(None, &privacy_settings)` checks the blocklist against `None`, returning `false` (do not skip).
  6. The master password is plain-text decoded, hashed, stored in SQLite `items`, and indexed in `items_fts`.

---

- **Resolution**: `should_skip` now matches on `foreground_exe` and fails CLOSED on `None`, skipping the capture and logging at warn. Deliberately not a setting: losing an occasional capture from a process we cannot name is not comparable to storing a password, and a switch would only invite turning the protection off. `get_foreground_process_name` also distinguishes the three cases in its logs — no foreground window (debug, common and harmless), OpenProcess denied (warn, unusual since `PROCESS_QUERY_LIMITED_INFORMATION` is designed to work across integrity levels), and a name that resolves. Regression test `unidentified_process_fails_closed`. The pre-existing `test_process_blocklist_case_insensitive` asserted the fail-open behaviour and was corrected.

### 2. Blocklist comparison fails when user configures full executable paths
- **File & Line**: `src-tauri/src/clipboard/privacy.rs:59-69`
- **Category**: 3 — The Privacy Filter
- **Severity**: High
- **Assignment**: Assigned to clipboard worker.
- **What breaks**: `get_foreground_process_name()` strips the directory path and returns only the file name (e.g. `"keepass.exe"`). `should_skip()` compares `blocked.to_ascii_lowercase() == exe_lower`. If a user added an executable by browsing to its path in the settings UI (as specified in SPEC §2.3), `blocked` contains a full path (e.g. `"C:\Program Files\KeePass\keepass.exe"`), causing the string comparison to fail.
- **Concrete Failure Scenario**:
  1. User adds a process via the Settings file picker: `settings.blocked_processes` contains `"C:\Program Files\1Password\1password.exe"`.
  2. User copies a credential in 1Password.
  3. `get_foreground_process_name()` returns `Some("1password.exe")`.
  4. `should_skip()` evaluates `"c:\\program files\\1password\\1password.exe" == "1password.exe"`, which returns `false`.
  5. The capture bypasses the privacy filter and is written to the database.

---

- **Resolution**: Added `exe_key`, which trims whitespace and quotes and reduces both the blocklist entry and the identified process to a lowercased file name, so a full path and a bare name are the same entry. Regression test `blocklist_matches_paths_and_bare_names` covers a full path in the blocklist, a mixed-case bare name, a padded entry, and a full path arriving from identification.

### 3. AB-BA Deadlock between `store.switch_root()` (relocation) and UI store queries
- **File & Line**: `src-tauri/src/store/mod.rs:124-132`, `src-tauri/src/store/mod.rs:291-301`, `src-tauri/src/store/mod.rs:303-307`
- **Category**: 4 — Transaction and Lock Discipline
- **Severity**: High
- **What breaks**: Inverted lock acquisition order between `RwLock<PathBuf>` (`self.inner.root`) and `Mutex<Connection>` (`self.inner.conn`).
  - In `list()`, `search()`, `get()`, `stats()`, and `clear_history()`: locks `conn` (`self.conn()`) FIRST, then acquires `root` read lock (`self.root()`).
  - In `switch_root()`: acquires `root` write lock (`self.inner.root.write()`) FIRST, then locks `conn` (`self.inner.conn.lock()`).
- **Concrete Failure Scenario**:
  1. Thread A (User triggers Store Relocation in settings): `janitor::relocate` calls `store.switch_root()`.
  2. Thread A acquires `self.inner.root.write()` write lock.
  3. Concurrently, Thread B (Popup UI rendering items) calls `store.list()`.
  4. Thread B acquires `self.conn()` mutex lock.
  5. Thread B then calls `self.root()` (attempting to acquire `inner.root.read()`), blocking because Thread A holds the write lock.
  6. Thread A advances to `let mut conn_guard = self.inner.conn.lock();`, blocking because Thread B holds the `conn` mutex lock.
  7. **Result**: Complete deadlock; the UI thread and relocation thread freeze permanently.
- **Resolution**: Enforced consistent lock acquisition hierarchy across all store entry points (`list`, `search`, `get`, `blob_path`, `formats`, `delete`, `add_references`, `stats`, `clear_history`, `insert_capture`): always acquire `root` (`RwLock` read lock) before acquiring `conn` (`Mutex` lock). Documented locking order invariants at `StoreInner`. Added regression test `test_regression_finding_3_deadlock_switch_root_concurrent_queries`.

---

### 4. Non-transactional `delete_items` race condition creating orphan rows with missing blobs
- **File & Line**: `src-tauri/src/store/queries.rs:434-454`, `src-tauri/src/store/mod.rs:171-195`
- **Category**: 2 — Dedup and Refcount Rules
- **Severity**: Medium-High
- **What breaks**: In `delete_items`, the database row deletion and blob unlinking are executed outside a database transaction. A concurrent `insert_capture` for the same hash between the row deletion and `delete_blob_if_unreferenced` results in an unlinked blob file for the newly inserted row.
- **Concrete Failure Scenario**:
  1. Thread A (User deletes Item 1 with hash `H`):
      - Executes `conn.execute("DELETE FROM items WHERE id = ?1", [item1.id])`.
      - Thread A is preempted before executing `delete_blob_if_unreferenced`.
  2. Thread B (Clipboard listener captures identical content with hash `H`):
      - Executes duplicate check: `SELECT id FROM items WHERE hash = 'H'`. Returns `None` because Item 1 was deleted.
      - Calls `write_blob()`: sees `<root>/blobs/ab/cd/H` already on disk and reuses it without rewriting.
  3. Thread A resumes:
      - Calls `delete_blob_if_unreferenced(&conn, &root, 'H', ...)`: runs `SELECT COUNT(*) FROM items WHERE hash = 'H'`.
      - Because Thread B has not yet executed its `INSERT INTO items`, the query returns `0`.
      - Thread A deletes `<root>/blobs/ab/cd/H` from disk.
  4. Thread B resumes:
      - Executes `INSERT INTO items (..., hash='H', blob_path='ab/cd/H', ...)`.
  5. **Result**: Database row created by Thread B references a missing blob that was deleted out from under it.
- **Resolution**: Wrapped `DELETE FROM items` and derived blob refcount verification within an atomic SQLite write transaction `tx`. If the count of remaining references from `items` and `item_formats` reaches zero, unlinking the file is staged and only executed after the transaction successfully commits. Added regression test `test_regression_finding_4_delete_insert_race_transactional`.

---

### 5. Derived refcount deletion in `delete_items` ignores `item_formats` blob references
- **File & Line**: `src-tauri/src/store/blobs.rs:167-197`, `src-tauri/src/store/queries.rs:434-454`
- **Category**: 2 — Dedup and Refcount Rules
- **Severity**: Medium
- **What breaks**: `delete_blob_if_unreferenced` checks only `SELECT COUNT(*) FROM items WHERE hash = ?1`. Large format payloads (>64KB HTML/RTF stored in `item_formats.blob_path`) share the blob storage fanout tree. If an item format references a blob that is deleted via `delete_blob_if_unreferenced`, the format blob is deleted while other `item_formats` records still reference it.
- **Concrete Failure Scenario**:
  1. Item 1 is captured with a 100KB formatted HTML payload, written to blob `blobs/ab/cd/H_format` and recorded in `item_formats`.
  2. An item with primary hash `H_format` or an item containing that format is deleted.
  3. `delete_blob_if_unreferenced` queries `SELECT COUNT(*) FROM items WHERE hash = 'H_format'`.
  4. It returns `0` (because `item_formats` is not included in the count), and the blob file is unlinked from disk.
  5. When Item 1 is pasted with rich text formatting (`store.formats(1)`), `std::fs::read` fails with `NotFound`.
- **Resolution**: Updated `delete_blob_if_unreferenced` in `blobs.rs`, `queries.rs`, and the janitor startup sweep to query total references across both `items` and `item_formats` (`SELECT COUNT(*) FROM items WHERE hash = ?1` + `SELECT COUNT(*) FROM item_formats WHERE blob_path = ?1`). Added regression test `test_regression_finding_5_item_formats_blob_refcounting` verifying >64KB HTML payload survival across item deletion.

---

### 6. Sequence number tracking skipped on clipboard write error paths
- **File & Line**: `src-tauri/src/clipboard/writer.rs:86-92`, `src-tauri/src/clipboard/writer.rs:220-226`
- **Category**: 1 — Resource Leaks & Win32 State
- **Severity**: Medium
- **Assignment**: Assigned to clipboard worker.
- **What breaks**: `write_items` calls `EmptyClipboard()` on line 86, which generates a `WM_CLIPBOARDUPDATE` event in Windows. If `store.get(id)?` on line 92 or a subsequent conversion fails, the function returns early with `?`. The sequence number recording on line 225 is never reached.
- **Concrete Failure Scenario**:
  1. A paste command is sent for an item ID that was just deleted or corrupted.
  2. `write_items` opens the clipboard and calls `EmptyClipboard()`.
  3. `store.get(id)?` fails with `AppError::NotFound`.
  4. `write_items` returns `Err` without calling `record_sequence(seq)`.
  5. Windows fires `WM_CLIPBOARDUPDATE` to Rebuffer's listener thread.
  6. `is_our_sequence(seq)` returns `false` (old sequence number).
  7. The listener interprets the empty clipboard state as an external update and attempts to decode it.

---

- **Resolution**: `ClipboardGuard` carries a second flag, set immediately after `EmptyClipboard` succeeds, and its `Drop` records the clipboard sequence number whenever that flag is set. Recording therefore happens on the error path too, so an early `?` return can no longer leave the listener treating our own emptied clipboard as an external change.

### 7. Hourly background janitor thread hardcodes `max_store_bytes = None`
- **File & Line**: `src-tauri/src/store/mod.rs:94-114`
- **Category**: 7 — The Janitor's Size Cap
- **Severity**: Medium
- **What breaks**: The hourly background janitor thread spawned in `Store::open` invokes `janitor::run_cleanup(&store, None, None)`. The second argument (`max_store_bytes`) is hardcoded to `None` instead of reading the user's `settings.storage.max_store_bytes`.
- **Concrete Failure Scenario**:
  1. User configures a storage cap (e.g. `settings.storage.max_store_bytes = 2 * 1024 * 1024 * 1024` / 2 GB).
  2. Total store size grows to 8 GB.
  3. The hourly janitor thread triggers every 60 minutes, executing `run_cleanup(&store, None, None)`.
  4. Size cap pruning (`if let Some(cap) = max_store_bytes`) is skipped entirely because `cap` is `None`.
  5. Storage continues growing indefinitely until manual user intervention via the Settings Data tab.
- **Resolution**: Added `RetentionPolicy` field and `RwLock<RetentionPolicy>` inside `StoreInner`, initialized at open to `Default` (30 days, no byte cap). Added `pub fn set_retention_policy(&self, policy: RetentionPolicy)` and `pub fn retention_policy(&self) -> RetentionPolicy`. Updated hourly janitor thread and `Store::run_cleanup` to read the active policy. Implemented dual `storage-warning` event emission (once at 90% threshold with `removed_items: 0` before deleting, and after pruning). Added regression test `test_regression_finding_7_retention_policy_and_size_cap`.

---

### 8. SQLite connection mutex held across blocking disk `fsync` syscalls in `insert_capture`
- **File & Line**: `src-tauri/src/store/mod.rs:168-195`
- **Category**: 4 — Transaction and Lock Discipline / Latency
- **Severity**: Low-Medium
- **What breaks**: `let conn = self.conn();` is acquired at line 168 and held continuously while `write_blob()` performs file creation, disk writes, and synchronous `file.sync_all()?` (`fsync`).
- **Concrete Failure Scenario**:
  1. User copies a 100 MB file/image.
  2. `insert_capture` acquires the `conn` mutex lock.
  3. `write_blob` writes 100 MB to disk and blocks on `file.sync_all()?` for 150–500 ms.
  4. While the disk flush is executing, user presses hotkey (`Alt+V`) to show popup.
  5. The UI thread invokes `store.list()` / `store.tab_counts()`, blocking on `conn.lock()`.
  6. The popup window fails its < 80 ms responsiveness target.
- **Resolution**: Refactored `Store::insert_capture` to perform primary blob writes, thumbnail scheduling, and format blob writes entirely outside the SQLite `conn` mutex lock. The mutex lock is now taken only for duplicate checking and the final row insertion. Added regression test `test_regression_finding_8_fsync_lock_concurrency`.

---

## Category-by-Category Review Status

- **1. Resource Leaks on Error Path**: RAII `ClipboardGuard` properly releases `CloseClipboard()`. Win32 process handles in `privacy.rs` correctly invoke `CloseHandle()`. `GlobalUnlock` calls are present on all DIB/text early return branches.
- **2. Dedup & Refcount Rules**: Issues identified in findings **#4** and **#5** regarding non-transactional deletion races and format blob references.
- **3. Privacy Filter**: Issues identified in findings **#1** (failing open on elevation) and **#2** (exact path mismatch in blocklist).
- **4. Transaction and Lock Discipline**: Deadlock risk identified in finding **#3** and disk I/O lock contention identified in finding **#8**.
- **5. Size and Overflow Limits**: Header dimensions in DIB decoding are verified with overflow checks (`checked_mul`, `checked_add`). Text length and HDROP path sizes are clamped against `max_item_bytes`.
- **6. FTS5 Sync**: Triggers `items_fts_ai`, `items_fts_ad`, and `items_fts_au` correctly maintain external-content synchronization across insertions, deletions, renames, and duplicate timestamp updates.
- **7. Janitor's Size Cap**: Size cap pruning loop in `janitor::run_cleanup` terminates safely when all items are pinned/references without infinite loops or over-deletion; however, background hourly invocation misses the setting parameter (finding **#7**).
