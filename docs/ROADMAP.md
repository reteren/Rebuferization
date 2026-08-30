# Rebuffer — Roadmap

Eight phases. Each one ends with something you can actually run, so you never spend a week without visible progress. Estimates assume solo part-time work.

Checkbox convention: `[x]` = genuinely done against the current tree; `[ ]` = not done (or only partially, with a note).

---

## Phase 0 — Foundation (1–2 days)

Get an empty Tauri app that starts, hides to tray, and opens a window on a hotkey. No clipboard logic yet.

- [x] `npm create tauri-app@latest` — Svelte + TypeScript template
- [x] Add crates: `windows`, `rusqlite`, `arboard`, `image`, `blake3`, `window-vibrancy`, `serde`, `anyhow`, `tracing`
- [x] Add plugins: `global-shortcut`, `autostart`, `single-instance`, `notification`, `dialog`, `opener`
- [x] Tray icon with Settings / Enable-Disable entries
- [x] `Alt+V` registered, toggles a blank window
- [x] Logging to `%APPDATA%\Rebuffer\logs\` with rotation
- [x] `npm run tauri build` produces a working NSIS installer

**Done when:** the app starts silently to tray and `Alt+V` shows an empty window.

---

## Phase 1 — Capture and persist (3–5 days)

The engine. Still no real UI — verify with a debug list and by inspecting the database.

- [x] Hidden message window + `AddClipboardFormatListener`
- [x] `WM_CLIPBOARDUPDATE` handler with open-clipboard retry loop
- [x] Self-write detection via clipboard sequence number
- [x] Format decoders: `CF_UNICODETEXT`, `HTML Format`, `RTF`, `CF_DIBV5`/`CF_DIB`/`PNG`, `CF_HDROP`
- [x] **Privacy filter** — `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, process blocklist
- [x] SQLite schema + migrations (`schema.sql`), WAL mode
- [x] Content-addressed blob store with atomic temp-write-then-rename
- [x] BLAKE3 hashing and duplicate bump
- [x] 256 MB item cap, checked before allocation
- [x] Source-app detection via `GetForegroundWindow` → PID → process name
- [x] Startup integrity sweep for orphan rows and orphan blobs

**Done when:** you copy 50 mixed things, kill the process with Task Manager, restart, and everything is still in the database.

---

## Phase 2 — Popup window that works (4–6 days)

- [x] Window pre-created hidden at startup, shown on hotkey (measure the latency)
- [x] Cursor-relative positioning with work-area clamping and per-monitor DPI
- [x] Close on outside click and on `Esc`
- [x] Cache the previous foreground `HWND`
- [x] Acrylic/Mica via `window-vibrancy` with a Windows 10 fallback
- [x] Basic virtualized grid rendering real items
- [x] Click → write to clipboard → close
- [x] Keyboard navigation: arrows, Enter, Esc
- [x] Decide the activate vs `WS_EX_NOACTIVATE` question here

**Done when:** the loop copy → `Alt+V` → click → paste works end to end and feels instant.

---

## Phase 3 — Cards and visual identity (5–7 days)

This is where it starts looking like your reference image.

- [x] Design tokens: glass surfaces, borders, shadows, accent, type scale
- [x] Thumbnail pipeline — WebP, 512 px long edge, generated off the main thread
- [x] Text card: 7 lines × 15 chars, word-boundary wrapping, mid-word break for long words
- [x] Image, GIF (animated), video, file cards — *video card shows a play overlay, no shell thumbnail*
- [x] Link cards with favicon and domain; color cards with a swatch
- [x] Age badge, top-left
- [x] Format label, bottom-left, size-configurable
- [x] Hover: `scale(0.97)` + white overlay + border lift, `transform`/`opacity` only
- [x] Zoom dial in the corner — drag, hover-scroll, and `Ctrl`+wheel
- [x] Sticky date group headers (Today / Yesterday / dates)
- [x] Empty state and loading skeletons

**Done when:** a screenshot of the app looks like something you'd want to post.

---

## Phase 4 — Browsing power (4–6 days)

- [x] Tabs: All / Images / Text / Links / Files / Pinned
- [x] FTS5 search with 120 ms debounce and a `LIKE` fallback for short queries
- [x] Sorting: newest, oldest, name, size
- [x] Type filter plus extension facets with live counts
- [x] Pin / unpin, exempt from the janitor
- [x] Multi-select: `Ctrl+click`, `Shift+click`, `Ctrl+A`, and batch copy / delete / pin
- [x] Custom context menu with all entries from the spec
- [x] Rename (display title only)
- [x] Save as, Open, Open with, Show in folder

**Done when:** you can find any item from the last 30 days in under three seconds.

---

## Phase 5 — Shelf and drag-out (2–3 days)

- [x] **+ Add** button with a file picker
- [x] Reference items — path only, no copy, never expire, marked visually
- [x] Missing-file state with a remove action
- [x] OLE drag-out with `CF_HDROP`, materializing blobs to temp files with sensible names
- [x] Multi-item drag

**Done when:** you can drag three images out of the popup straight into Discord.

---

## Phase 6 — Settings and lifecycle (4–5 days)

- [x] `settings.json` with validation, defaults, and hot reload
- [x] Settings window: General / Storage / Appearance / Privacy / Data / About
- [x] Hotkey capture field rejecting reserved combinations
- [x] Aggressive `Win+V` mode behind a warning, with the low-level hook
- [ ] Optional helper to disable Windows' built-in clipboard history — *not built*
- [x] Retention janitor (1–30 days), hourly plus on startup
- [x] Storage cap with 90% warning toast and oldest-first pruning
- [x] Storage usage meter split by type, with "Clean now"
- [x] Store relocation with progress, verification, and rollback
- [x] Export / import `.rbx` with merge or replace
- [x] Auto-paste option and plain-text paste (plus `Shift`+click)
- [x] Silent autostart

**Done when:** every setting in the spec actually changes behaviour, and the app survives changing all of them at once.

---

## Phase 7 — Hardening and release (3–5 days)

- [ ] Multi-monitor testing, including mixed DPI and vertical arrangements — *positioning is unit-tested incl. negative coordinates and clamping, but never run on real mixed-DPI hardware*
- [x] 10,000-item stress test — scroll, search, and startup time — *measured in `docs/PERF.md` and `docs/PERF-UI.md`; the grid rendering bugs it exposed have been fixed*
- [ ] Copy from RDP sessions, VMs, and fullscreen games — *not tested*
- [ ] Behaviour when the store volume is full or disconnected — *not tested*
- [x] Database corruption recovery — *startup integrity sweep, orphan cleanup, corrupt-settings handling*
- [ ] Windows 10 fallback path verified on a real Win10 machine — *the flat-backdrop fallback exists; never run on real Win10*
- [ ] Memory-leak check over a 24-hour run — *not done*
- [ ] README screenshots and a short demo GIF — *screenshots shipped under `docs/img/`; demo GIF pending*
- [x] GitHub Actions build workflow producing an NSIS installer artifact
- [ ] Tagged v1.0.0 release with a SmartScreen note in the release body — *not done*

---

## Later, if you want it

- OCR on screenshots with the Windows 11 native API — makes images searchable, something `Win+V` cannot do
- Optional DPAPI encryption for text blobs
- Snippets tab with reusable templates
- Per-application rules (e.g. always paste plain text into this app)
- Paste stack: copy three things, press `Ctrl+V` three times, get them in order

---

## Build order rationale

Capture comes before UI because a beautiful window over a broken engine is a rewrite; a plain window over a solid engine is an afternoon of CSS. The privacy filter is in Phase 1 rather than Phase 6 because every day it's missing is a day of passwords accumulating in a file you'll have to explain deleting. The zoom dial and glass work land in Phase 3, once real data exists to look at, because designing card layouts against fake data always produces cards that break on real content.