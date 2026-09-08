# Rebuffer — Technical Specification

Version 0.1 · Windows only · Tauri 2 + Rust + Svelte 5

---

## 1. Architecture

Three long-lived pieces run inside one process:

**Message-loop thread (Rust).** Owns a hidden `HWND` created only to receive Windows messages. It registers a clipboard listener with `AddClipboardFormatListener` and handles `WM_CLIPBOARDUPDATE`, plus `WM_HOTKEY` when the simple hotkey path is used. No polling anywhere.

**Store (Rust).** SQLite connection pool in WAL mode, plus a blob directory. Every capture is written synchronously before the capture handler returns, so a force-kill can lose at most the item currently being copied.

**UI (WebView2).** Two windows: the popup and settings. Both are created hidden at startup and only shown/hidden afterwards, because creating a WebView2 window takes 300–600 ms and the popup must appear in under ~80 ms.

```
clipboard change ──► WM_CLIPBOARDUPDATE
                         │
                    privacy check ──► drop
                         │
                     decode formats
                         │
                     hash content ──► duplicate? ──► bump timestamp, done
                         │
                 write blob + thumbnail
                         │
                    INSERT into SQLite
                         │
                  emit "item-added" ──► UI updates if visible
```

---

## 2. Clipboard capture

### 2.1 Listener

Register `AddClipboardFormatListener(hwnd)` at startup, `RemoveClipboardFormatListener` on shutdown. On `WM_CLIPBOARDUPDATE`:

1. Ignore the event if we set the clipboard ourselves (compare against a sequence number stored on our last write, via `GetClipboardSequenceNumber`).
2. Open the clipboard with retry — other apps hold it briefly. Retry up to 10 times with 20 ms backoff, then give up silently.
3. Run the privacy check (§2.3).
4. Enumerate formats, decode, store.

### 2.2 Formats and what we keep

Windows exposes the same content in several formats at once. We keep a **primary kind** for display plus every format needed to restore the clipboard faithfully.

| Priority | Windows format | Stored as | Kind |
|---|---|---|---|
| 1 | `CF_HDROP` | list of paths (reference) | `file` |
| 2 | `CF_DIBV5` / `CF_DIB` / `PNG` | PNG or original bytes | `image` |
| 3 | `HTML Format` | HTML blob + extracted plain text | `text` (rich) |
| 4 | `Rich Text Format` | RTF blob + plain text | `text` (rich) |
| 5 | `CF_UNICODETEXT` | UTF-8 text | `text` |

**Decision on rich text (your question 6):** always store `CF_UNICODETEXT` as the canonical searchable text, and *additionally* store HTML/RTF blobs when present. On paste, restore all stored formats so Word-to-Word keeps its formatting. Settings expose `pasteAsPlainText` (default `false`) and holding `Shift` while clicking pastes plain text regardless. This is the best of both and costs almost nothing in storage.

Sub-kinds derived at capture time for tab filtering:
- `link` — the text is a single well-formed URL
- `color` — the text matches `#RGB`, `#RRGGBB`, `rgb(...)`, `hsl(...)`
- `code` — heuristic: >30% of lines start with indentation, or it parses as JSON/XML
- `image/animated` — GIF or animated WebP
- `video` — file reference with a video extension

### 2.3 Privacy filtering

Skip the capture entirely if **any** of these hold:

- The registered format `ExcludeClipboardContentFromMonitorProcessing` is present
- The registered format `CanIncludeInClipboardHistory` is present and its DWORD value is `0`
- `CanUploadToCloudClipboard` is present with value `0` *(logged but not blocking — configurable)*
- The foreground process at capture time matches an entry in `settings.blockedProcesses`

Default blocklist ships with: `keepass.exe`, `keepassxc.exe`, `1password.exe`, `bitwarden.exe`, `lastpass.exe`, `dashlane.exe`, `protonpass.exe`. Settings UI lets you add processes by picking a running process or browsing to an executable.

This is not optional and not deferrable — without it the database becomes a plaintext password log.

### 2.4 Size limit

Items above `settings.maxItemBytes` (default 256 MB) are discarded with no database entry. Check the size *before* copying blob bytes: for `CF_DIB` compute expected size from the header, for `CF_HDROP` stat the files.

### 2.5 Deduplication

Content hash is BLAKE3 over the primary blob (or over the normalized text for text items). Unique index on `hash`. On collision: `UPDATE items SET created_at = now(), copy_count = copy_count + 1` — the item jumps to the top of the list, no duplicate row.

File references hash the *path list*, not the file contents, so re-copying the same file from Explorer bumps rather than duplicates.

---

## 3. Storage

### 3.1 Layout decision (your questions 8 and 9)

**Metadata in SQLite, blobs on disk.** SQLite is excellent up to roughly 1 MB per row; a 256 MB screenshot inside a row would bloat the file, slow every query, and make VACUUM painful. So:

- `rebuffer.db` — rows, search index, settings pointer. Typically under 20 MB even with 30 days of history.
- `blobs/ab/cd/<hash>` — content-addressed, two-level fanout to keep directories small. Content addressing gives deduplication for free (question 11): identical images written twice occupy one file, and the second `INSERT` just reuses the hash.
- `blobs/thumbs/<hash>.webp` — max 512 px on the long edge, quality 80. Animated GIFs get both a static first-frame WebP and keep the original for playback.

**Encryption decision:** off by default. SQLCipher would add a dependency, slow every read, and require key storage that on a single-user desktop reduces to DPAPI — which protects against another user account, not against malware running as you. The store folder inherits `%APPDATA%` ACLs, which already blocks other users. Instead: document the risk in the README, ship the privacy filter and process blocklist as the real defense, and leave an `encryptTextBlobs` DPAPI option for a later phase if you want it.

### 3.2 Durability (your question 7)

- `PRAGMA journal_mode = WAL`
- `PRAGMA synchronous = NORMAL`
- Blob is written to a temp file, `fsync`'d, then atomically renamed into place, and only then is the row inserted. A crash mid-write leaves an orphan temp file, never a row pointing at a missing blob.
- Startup runs an integrity sweep: rows with missing blobs are deleted, blobs with no row are deleted.

### 3.3 Schema

See `schema.sql`. Summary:

- `items` — one row per clipboard entry (kind, sub_kind, hash, sizes, timestamps, pin state, source app, reference path)
- `item_formats` — extra clipboard formats belonging to an item (HTML, RTF, secondary image encodings)
- `item_files` — the individual paths of a multi-file `CF_HDROP` capture
- `items_fts` — FTS5 external-content table over `preview_text`, kept in sync by triggers
- `meta` — schema version and bookkeeping

### 3.4 Retention (your questions 4, 5)

Two independent janitor rules, both skipping pinned items and manual shelf items:

1. **Age** — delete where `created_at < now - settings.retentionDays` (1–30, default 30). Runs on startup and hourly.
2. **Size cap** — if `settings.maxStoreBytes` is set and total blob size exceeds it, delete oldest-first until under 90% of the cap. Fires a Windows toast: *"Clipboard store full — removed 47 old items to free 1.2 GB."* Also warns once at 90% before deleting anything.

Deletion is: remove row → decrement blob refcount → delete blob if refcount hits zero. Refcount is derived (`SELECT COUNT(*) FROM items WHERE hash = ?`), not stored.

### 3.5 Store relocation (your question 10)

Changing the store path copies `rebuffer.db` and the whole `blobs/` tree to the new location, verifies row count and total byte size, then deletes the old tree. Progress dialog with a cancel that rolls back. Refuse if the target volume has less free space than the current store size × 1.2.

---

## 4. Hotkey (your question 1 context)

**Default: `Alt+V`.** Registered with `RegisterHotKey(hwnd, id, MOD_ALT, 'V')`.

**`Win+V` is a system-reserved combination** — `RegisterHotKey` fails on any `MOD_WIN` chord that Windows already claims. Two paths are offered in settings:

- *Standard mode* (default) — `RegisterHotKey`, any combination not owned by the system. Cheap, reliable, invisible to antivirus.
- *Aggressive mode* (opt-in, with an explicit warning) — a `WH_KEYBOARD_LL` hook that swallows the `Win+V` keystroke before the shell sees it. Caveats shown in the UI: some antivirus products flag low-level keyboard hooks, and the hook callback must return within the system timeout or Windows silently unregisters it. The hook does nothing but compare a key combination and post a message — no logging, ever.

Aggressive mode also offers a one-click helper that disables Windows' own clipboard history (`HKCU\Software\Microsoft\Clipboard\EnableClipboardHistory = 0`), since running both is confusing.

Hotkey capture UI: a field that records the next chord pressed, rejects reserved combinations with an inline explanation, and warns on single-key or `Ctrl+C`-style bindings.

---

## 5. Window behaviour

### 5.1 Positioning (your question 12)

On show:
1. `GetCursorPos` → find the monitor containing that point → `GetMonitorInfo` for its **work area** (excludes the taskbar).
2. Default size = 40% of that monitor's work area, or the user's saved size.
3. Place the window's top-left at the cursor, then clamp: if `x + w > workArea.right`, set `x = workArea.right - w`, same for the bottom edge, and never above `workArea.left/top`.
4. Multi-monitor with different DPI: read the per-monitor DPI and scale before clamping, or the window lands half off-screen.

### 5.2 Focus and dismissal (your question 22)

- The window is created with `WS_EX_NOACTIVATE` so showing it does not steal focus from whatever you were typing in. Before showing, cache `GetForegroundWindow()` as the paste target.
- Clicking anywhere outside the window closes it immediately (`WM_ACTIVATE` / `WM_KILLFOCUS` handler), matching `Win+V`. Also `Esc`.
- Because the window never activates, keyboard input needs a raw input or hook path while it's visible — arrows and typing must reach the popup without a normal focus grab. Simplest workable approach: activate the window but restore the cached foreground window on close, and keep the cached HWND for paste injection.

### 5.3 Glass appearance

Order matters:
1. `tauri.conf.json`: `"transparent": true`, `"decorations": false`, `"shadow": true`
2. `window-vibrancy`: apply `apply_acrylic` on Windows 11 (or Mica with `DWMSBT_TRANSIENTWINDOW`), fall back to a flat `rgba(18,20,24,0.92)` background on Windows 10
3. CSS on top: layered radial gradients, a 1 px `rgba(255,255,255,0.08)` inner border, and `backdrop-filter: blur()` only on inner panels, not the root

Do not rely on `backdrop-filter` for the window backdrop — WebView2 blurs against the page, not the desktop.

### 5.4 Paste-back (your question 17)

Click → write to clipboard → close window → restore cached foreground window with `SetForegroundWindow`. If `settings.autoPaste` is on: after a 30–50 ms settle delay, send `Ctrl+V` with `SendInput`. Default off, since auto-paste into the wrong window is destructive.

Caveat to surface in the settings tooltip: if the target app runs elevated and Rebuffer does not, `SendInput` is blocked by UIPI and nothing happens. The clipboard write still succeeds, so the user can paste manually.

---

## 6. UI

### 6.1 Layout

```
┌────────────────────────────────────────────────────────────┐
│  [+ Add]   All · Images · Text · Links · Files · Pinned     │  tabs
│  [search…]                         [sort ▾]  [filter ▾]     │  toolbar
├────────────────────────────────────────────────────────────┤
│  Today                                                      │  date group
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐          │
│  │14m │ │    │ │    │ │    │ │    │ │    │ │    │          │
│  │ PNG│ │ TXT│ │ ...│ │    │ │    │ │    │ │    │          │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘          │
│  Yesterday                                                  │
│  ...                                                        │
├────────────────────────────────────────────────────────────┤
│  1,284 items · 3.2 GB                              ◯ zoom   │  status bar
└────────────────────────────────────────────────────────────┘
```

### 6.2 Grid and zoom (your question 19)

Default tile size targets **20 columns × 7 visible rows** at 2560×1440 in a window sized to 40% of the work area — matching Explorer's Large Icons. Zoom dial in the bottom-right corner:
- Drag it, or hover it and scroll
- `Ctrl` + wheel anywhere in the grid also zooms
- Five steps, persisted per user
- Rendering uses a virtualized grid (`svelte-virtual` or hand-rolled) so 10,000 items scroll at 60 fps

### 6.3 Cards

Each card carries:
- **Top-left:** relative age — `14m`, `3h`, `2d`. Toggleable, `settings.showAge`
- **Bottom-left:** format label in large type — `PNG`, `TXT`, `MP4`. Size configurable (`settings.formatLabelSize`: off / small / medium / large)
- **Center:** the preview

Preview by kind:
- *Image* — thumbnail, `object-fit: cover`
- *GIF* — animated by default, `settings.animateGifs` swaps to static first frame
- *Video* — first-frame thumbnail with a play glyph
- *Text* — a rounded panel with **7 lines × ~15 characters**, wrapping on word boundaries, breaking mid-word only when a single word exceeds 15 characters. Ellipsis on overflow. Monospace for `code` sub-kind
- *Link* — favicon plus the domain, with the page title if we have it
- *Color* — a filled swatch with the hex value
- *File* — the shell icon for that extension plus the filename

Hover: `transform: scale(0.97)` (the "press down" effect) plus a white overlay at 6% opacity and a brighter border. Animate `transform` and `opacity` only — never `width`/`height`/`box-shadow` on a list this size. Respect `prefers-reduced-motion`.

### 6.4 Date grouping and sorting (your question 13)

Sticky group headers: `Today`, `Yesterday`, then explicit dates (`27 August`, then `12 March 2026` once the year differs).

Sort options: Newest (default), Oldest, Name A→Z, Name Z→A, Size ↑, Size ↓. Grouping applies only to the date sorts; other sorts render a flat list.

Filters: a type dropdown (All / Images / Text / Links / Files / Video / Other) plus an extension sub-filter populated from what is actually in the store (`PNG (412)`, `TXT (88)`).

Search: FTS5 over `preview_text` and filename, with a plain `LIKE` fallback for prefixes under 3 characters. Debounced 120 ms.

### 6.5 Selection and keyboard (your questions 4, 14)

- Arrows move focus, `Enter` copies, `Esc` closes
- `Ctrl+click` toggles individual items, `Shift+click` selects a range, `Ctrl+A` selects all in view
- Multi-select actions: copy all (images concatenate as a file list; text joins with newlines), delete, pin, drag out together
- Typing any printable character focuses the search box

### 6.6 Context menu (your question 20)

Custom menu, not the shell menu. Entries, adapting to item kind:

| Entry | Applies to |
|---|---|
| Copy | all |
| Copy as plain text | rich text |
| Pin / Unpin | all |
| Open | image, video, file |
| Open with… | image, video, file |
| Save as… | all (text saves as `.txt`) |
| Show in folder | file references, and stored blobs (opens the store folder with the blob selected) |
| Rename | all — renames the display title, not the source file |
| Delete | all |

`Show in folder` uses `SHOpenFolderAndSelectItems`, not `explorer /select`, so it reuses an existing window. `Open with…` invokes the shell's Open With dialog via `SHOpenWithDialog`.

### 6.7 Manual shelf (your questions 22, 24)

The **+ Add** button in the top-left opens a file picker. Added items are stored **by reference** — the path is recorded, the bytes are not copied. Consequences to handle:
- They never expire and are exempt from the janitor
- They are marked with a small pin-like glyph to distinguish them from captures
- If the source file disappears, the card renders dimmed with a "missing" state and offers to remove it
- Copying one to the clipboard writes a `CF_HDROP` with the original path

Drag-and-drop *into* the window is intentionally absent, since the window closes on outside click. Dragging *out* is supported (§6.8).

### 6.8 Drag out (your question 23)

Starting a drag from a card initiates an OLE drag with `CF_HDROP` pointing at the blob (materialized to a temp file with a sensible name for non-file items) or at the original path for references. Dropping into Explorer, Discord, or an email client works as expected.

---

## 7. Settings

Stored as `settings.json` next to the database, hot-reloaded on change, validated on load with fallback to defaults.

```jsonc
{
  "version": 1,
  "hotkey": { "binding": "Alt+V", "aggressiveMode": false },
  "storage": {
    "path": "%APPDATA%\\Rebuffer",
    "retentionDays": 30,          // 1–30
    "tempFilesDays": 7,           // 1–90; how long a file extracted from an item is kept
    "maxItemBytes": 268435456,    // 256 MB
    "maxStoreBytes": null,        // null = unlimited; warns at 90%, prunes at 100%
    "notifyWhenFull": true
  },
  "window": {
    "sizeMode": "percent",        // "percent" | "fixed"
    "percentOfMonitor": 40,
    "fixed": { "width": 1100, "height": 700 },
    "zoomStep": 3                 // 1–5
  },
  "behavior": {
    "autoPaste": false,
    "pasteAsPlainText": false,
    "closeOnCopy": true,
    "launchOnStartup": true,
    "silentStart": true,
    "captureEnabled": true
  },
  "appearance": {
    "showAge": true,
    "formatLabelSize": "medium",  // "off" | "small" | "medium" | "large"
    "animateGifs": true,
    "reduceMotion": false,
    "accent": "#7aa2ff"
  },
  "privacy": {
    "respectClipboardFlags": true,
    "blockedProcesses": ["keepass.exe", "keepassxc.exe", "1password.exe", "bitwarden.exe"]
  }
}
```

Settings window sections: **General** (hotkey, startup, capture toggle), **Storage** (path, retention, caps, usage meter with a "Clean now" button), **Appearance** (window size, zoom, badges, animation), **Privacy** (flags, process blocklist), **Data** (export, import, reset), **About**.

Storage usage meter (idea 6): a bar showing total size split by type — images / videos / text / files — with item counts and a "Clean items older than [N] days now" action.

### Export / import (your question 29)

Export writes a `.rbx` file — a zip containing `settings.json`, a `manifest.json`, `items.jsonl`, and the `blobs/` tree. Import offers *merge* (skip hashes already present) or *replace*. Warn on export that the archive contains full clipboard history in the clear.

---

## 8. Tray

Two entries only, per spec: **Settings** and **Enable/Disable**. The enable toggle stops the clipboard listener but leaves the hotkey and history intact — you can still browse what you already have. Tray icon renders in a muted variant while disabled. Left-click opens the popup at the cursor; right-click opens the menu.

---

## 9. IPC surface

Tauri commands exposed to the frontend:

```rust
// query
list_items(filter: Filter, sort: Sort, offset: u32, limit: u32) -> Vec<ItemDto>
search_items(query: String, filter: Filter, limit: u32) -> Vec<ItemDto>
get_item_blob_url(id: i64) -> String        // asset protocol URL
get_extension_facets(filter: Filter) -> Vec<Facet>
get_storage_stats() -> StorageStats

// mutate
copy_to_clipboard(ids: Vec<i64>, plain_text: bool) -> ()
paste_to_previous_window(ids: Vec<i64>, plain_text: bool) -> ()
set_pinned(ids: Vec<i64>, pinned: bool) -> ()
rename_item(id: i64, title: String) -> ()
delete_items(ids: Vec<i64>) -> ()
add_files(paths: Vec<String>) -> Vec<ItemDto>
save_item_as(id: i64, target: String) -> ()
open_item(id: i64) -> ()
open_item_with(id: i64) -> ()
show_in_folder(id: i64) -> ()
begin_drag(ids: Vec<i64>) -> ()

// settings & lifecycle
get_settings() -> Settings
update_settings(patch: SettingsPatch) -> Settings
relocate_store(path: String) -> ()           // emits progress events
export_data(path: String) -> ()
import_data(path: String, mode: ImportMode) -> ()
set_capture_enabled(enabled: bool) -> ()
run_cleanup_now(older_than_days: Option<u32>) -> CleanupResult
hide_popup() -> ()
```

Events emitted to the frontend: `item-added`, `item-updated`, `items-deleted`, `settings-changed`, `store-progress`, `storage-warning`.

`ItemDto`:

```ts
type ItemDto = {
  id: number
  kind: 'text' | 'image' | 'video' | 'file' | 'other'
  subKind: 'plain' | 'rich' | 'code' | 'link' | 'color' | 'animated' | null
  title: string | null
  previewText: string | null      // first ~200 chars, for text cards
  thumbUrl: string | null
  ext: string | null              // "PNG", "TXT" — shown on the card
  byteSize: number
  createdAt: number               // unix ms
  pinned: boolean
  isReference: boolean
  refPath: string | null
  sourceApp: string | null
  missing: boolean                // reference whose file is gone
}
```

---

## 10. Performance targets

| Metric | Target |
|---|---|
| Hotkey → window visible | < 80 ms |
| Idle RAM | < 60 MB |
| Idle CPU | 0% (event-driven, no polling) |
| Capture → row committed | < 40 ms for text, < 300 ms for a 10 MB image |
| Scroll with 10,000 items | 60 fps |
| Cold start to tray-ready | < 1.5 s |

Achieved by: pre-created hidden windows, virtualized grid, thumbnails instead of originals in the grid, `LIMIT`/`OFFSET` paging at 200 items, and moving image decode/encode to a worker thread pool.

---

## 11. Open decisions

- Whether the popup activates (simpler keyboard handling) or stays non-activating (no focus flicker). Prototype both in Phase 2 and pick by feel.
- Video thumbnails need a frame extractor. Shipping ffmpeg is 30+ MB; the Windows Shell thumbnail API (`IShellItemImageFactory`) is free and already knows every codec installed. Prefer the shell API.
- Whether `Show in folder` on a captured (non-reference) item should open the blob store at all, or be hidden for those items.
