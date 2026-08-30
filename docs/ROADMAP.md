# Rebuffer — Roadmap

Eight phases. Each one ends with something you can actually run, so you never spend a week without visible progress. Estimates assume solo part-time work.

---

## Phase 0 — Foundation (1–2 days)

Get an empty Tauri app that starts, hides to tray, and opens a window on a hotkey. No clipboard logic yet.

- [ ] `npm create tauri-app@latest` — Svelte + TypeScript template
- [ ] Add crates: `windows`, `rusqlite`, `arboard`, `image`, `blake3`, `window-vibrancy`, `serde`, `anyhow`, `tracing`
- [ ] Add plugins: `global-shortcut`, `autostart`, `single-instance`, `notification`, `dialog`, `opener`
- [ ] Tray icon with Settings / Enable-Disable entries
- [ ] `Alt+V` registered, toggles a blank window
- [ ] Logging to `%APPDATA%\Rebuffer\logs\` with rotation
- [ ] `npm run tauri build` produces a working NSIS installer

**Done when:** the app starts silently to tray and `Alt+V` shows an empty window.

---

## Phase 1 — Capture and persist (3–5 days)

The engine. Still no real UI — verify with a debug list and by inspecting the database.

- [ ] Hidden message window + `AddClipboardFormatListener`
- [ ] `WM_CLIPBOARDUPDATE` handler with open-clipboard retry loop
- [ ] Self-write detection via clipboard sequence number
- [ ] Format decoders: `CF_UNICODETEXT`, `HTML Format`, `RTF`, `CF_DIBV5`/`CF_DIB`/`PNG`, `CF_HDROP`
- [ ] **Privacy filter** — `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, process blocklist. Do this now, not later.
- [ ] SQLite schema + migrations (`schema.sql`), WAL mode
- [ ] Content-addressed blob store with atomic temp-write-then-rename
- [ ] BLAKE3 hashing and duplicate bump
- [ ] 256 MB item cap, checked before allocation
- [ ] Source-app detection via `GetForegroundWindow` → PID → process name
- [ ] Startup integrity sweep for orphan rows and orphan blobs

**Done when:** you copy 50 mixed things, kill the process with Task Manager, restart, and everything is still in the database.

---

## Phase 2 — Popup window that works (4–6 days)

- [ ] Window pre-created hidden at startup, shown on hotkey (measure the latency)
- [ ] Cursor-relative positioning with work-area clamping and per-monitor DPI
- [ ] Close on outside click and on `Esc`
- [ ] Cache the previous foreground `HWND`
- [ ] Acrylic/Mica via `window-vibrancy` with a Windows 10 fallback
- [ ] Basic virtualized grid rendering real items
- [ ] Click → write to clipboard → close
- [ ] Keyboard navigation: arrows, Enter, Esc
- [ ] Decide the activate vs `WS_EX_NOACTIVATE` question here

**Done when:** the loop copy → `Alt+V` → click → paste works end to end and feels instant.

---

## Phase 3 — Cards and visual identity (5–7 days)

This is where it starts looking like your reference image.

- [ ] Design tokens: glass surfaces, borders, shadows, accent, type scale
- [ ] Thumbnail pipeline — WebP, 512 px long edge, generated off the main thread
- [ ] Text card: 7 lines × 15 chars, word-boundary wrapping, mid-word break for long words
- [ ] Image, GIF (animated), video (shell thumbnail), file (shell icon) cards
- [ ] Link cards with favicon and domain; color cards with a swatch
- [ ] Age badge, top-left
- [ ] Format label, bottom-left, size-configurable
- [ ] Hover: `scale(0.97)` + white overlay + border lift, `transform`/`opacity` only
- [ ] Zoom dial in the corner — drag, hover-scroll, and `Ctrl`+wheel
- [ ] Sticky date group headers (Today / Yesterday / dates)
- [ ] Empty state and loading skeletons

**Done when:** a screenshot of the app looks like something you'd want to post.

---

## Phase 4 — Browsing power (4–6 days)

- [ ] Tabs: All / Images / Text / Links / Files / Pinned
- [ ] FTS5 search with 120 ms debounce and a `LIKE` fallback for short queries
- [ ] Sorting: newest, oldest, name, size
- [ ] Type filter plus extension facets with live counts
- [ ] Pin / unpin, exempt from the janitor
- [ ] Multi-select: `Ctrl+click`, `Shift+click`, `Ctrl+A`, and batch copy / delete / pin
- [ ] Custom context menu with all entries from the spec
- [ ] Rename (display title only)
- [ ] Save as, Open, Open with, Show in folder

**Done when:** you can find any item from the last 30 days in under three seconds.

---

## Phase 5 — Shelf and drag-out (2–3 days)

- [ ] **+ Add** button with a file picker
- [ ] Reference items — path only, no copy, never expire, marked visually
- [ ] Missing-file state with a remove action
- [ ] OLE drag-out with `CF_HDROP`, materializing blobs to temp files with sensible names
- [ ] Multi-item drag

**Done when:** you can drag three images out of the popup straight into Discord.

---

## Phase 6 — Settings and lifecycle (4–5 days)

- [ ] `settings.json` with validation, defaults, and hot reload
- [ ] Settings window: General / Storage / Appearance / Privacy / Data / About
- [ ] Hotkey capture field rejecting reserved combinations
- [ ] Aggressive `Win+V` mode behind a warning, with the low-level hook
- [ ] Optional helper to disable Windows' built-in clipboard history
- [ ] Retention janitor (1–30 days), hourly plus on startup
- [ ] Storage cap with 90% warning toast and oldest-first pruning
- [ ] Storage usage meter split by type, with "Clean now"
- [ ] Store relocation with progress, verification, and rollback
- [ ] Export / import `.rbx` with merge or replace
- [ ] Auto-paste option and plain-text paste (plus `Shift`+click)
- [ ] Silent autostart

**Done when:** every setting in the spec actually changes behaviour, and the app survives changing all of them at once.

---

## Phase 7 — Hardening and release (3–5 days)

- [ ] Multi-monitor testing, including mixed DPI and vertical arrangements
- [ ] 10,000-item stress test — scroll, search, and startup time
- [ ] Copy from RDP sessions, VMs, and fullscreen games
- [ ] Behaviour when the store volume is full or disconnected
- [ ] Database corruption recovery
- [ ] Windows 10 fallback path verified on a real Win10 machine
- [ ] Memory-leak check over a 24-hour run
- [ ] README screenshots and a short demo GIF
- [ ] GitHub Actions build workflow producing an NSIS installer artifact
- [ ] Tagged v1.0.0 release with a SmartScreen note in the release body

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
