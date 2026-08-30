# Rebuffer

> A fast, persistent clipboard history for Windows. Everything you copy stays for 30 days — text, images, GIFs, videos, files — and comes back with one hotkey.

Rebuffer is a replacement for the built-in Windows `Win+V` clipboard history, with a dark liquid-glass interface, 30-day persistence across reboots, search, sorting, filters, pinning, and a manual "shelf" where you can park files you use often.

*(Name is a placeholder — rename freely. It follows the `Re-` pattern of Renarrator / Rerounder.)*

---

## Features

**Capture**
- Records every clipboard change: Unicode text, rich text (HTML/RTF), images (PNG/JPG/GIF/WebP/BMP), videos, and file references
- Survives reboots, crashes, and force-kills — nothing is buffered in memory waiting to be written
- Duplicate detection: copying the same thing again bumps the existing entry to the top instead of creating a clone
- Respects clipboard privacy flags, so password managers never end up in your history
- Per-application blocklist for anything else you don't want recorded

**Browse**
- Opens next to your cursor, clamped so the window always fits on the monitor you opened it on
- Grid of cards, Explorer-style, with a zoom dial you can drag or scroll
- Grouped by day (Today, Yesterday, specific dates), like Explorer's date grouping
- Tabs: All / Images / Text / Links / Files / Pinned
- Search, sort (name, size, newest, oldest), and filter by type or exact extension
- Relative age badge on each card ("14m ago", "3h ago")
- Format label on each card (`PNG`, `TXT`, `MP4`)
- Full keyboard navigation — arrows, Enter, Esc, Ctrl/Shift multi-select
- Pin anything to keep it past the auto-clean window

**Use**
- Click to copy back to the clipboard, or enable auto-paste to drop it straight into whatever you were typing in
- Drag items out into Explorer or any other app
- Add files manually with the **+** button — those are stored by reference and never expire
- Right-click menu: Open, Open with, Save as, Show in folder, Rename, Pin, Delete

**Manage**
- Auto-clean after 1–30 days (configurable)
- Optional storage cap — when the store exceeds your limit, the oldest unpinned items are removed and you get a notification
- Tray icon with two entries: Settings, and Enable/Disable
- Silent autostart with Windows
- Export/import of history and settings

---

## Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | Tauri 2 | Native window, small binary, ~40 MB RAM idle |
| Backend | Rust | Direct WinAPI access for clipboard, hooks, and paste injection |
| Frontend | Svelte 5 + TypeScript + Vite | Fast cold start; the window must appear in under ~80 ms |
| Database | SQLite (WAL) via `rusqlite` | Crash-safe metadata + FTS5 full-text search |
| Blobs | Content-addressed files on disk | 256 MB items don't belong in a database row |

Windows only. Windows 10 1809+ supported, Windows 11 recommended (Mica/Acrylic backdrop).

Key crates: `windows`, `arboard`, `rusqlite`, `image`, `blake3`, `window-vibrancy`, `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, `tauri-plugin-single-instance`, `tauri-plugin-notification`.

---

## Getting started

### Prerequisites

1. **Rust** — install via [rustup](https://rustup.rs/), stable toolchain, `x86_64-pc-windows-msvc`
2. **Visual Studio Build Tools 2022** with the *Desktop development with C++* workload (Tauri needs the MSVC linker)
3. **Node.js 20+** and npm
4. **WebView2 Runtime** — preinstalled on Windows 11; on Windows 10 grab the Evergreen Bootstrapper from Microsoft

Verify:

```powershell
rustc --version
node --version
```

### Setup

```powershell
git clone https://github.com/<you>/rebuffer.git
cd rebuffer
npm install
npm run tauri dev
```

### Build a release installer

```powershell
npm run tauri build
```

Output lands in `src-tauri/target/release/bundle/nsis/`. The build is unsigned, so SmartScreen will warn on first run — that's expected for an unsigned open-source binary.

---

## Project layout

```
rebuffer/
├─ src/                     # Svelte frontend
│  ├─ routes/
│  │  ├─ Popup.svelte       # the Alt+V window
│  │  └─ Settings.svelte    # settings window
│  ├─ lib/
│  │  ├─ components/        # Card, Grid, Tabs, ZoomDial, ContextMenu, ...
│  │  ├─ stores/            # items, settings, selection
│  │  └─ styles/            # liquid-glass tokens
│  └─ main.ts
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ clipboard/         # listener, decoders, writer
│  │  ├─ hotkey/            # RegisterHotKey + optional LL hook
│  │  ├─ store/             # SQLite, blob store, janitor
│  │  ├─ window/            # positioning, vibrancy, focus handling
│  │  ├─ tray.rs
│  │  └─ commands.rs        # IPC surface
│  ├─ migrations/
│  └─ tauri.conf.json
├─ docs/
│  ├─ SPEC.md
│  └─ ROADMAP.md
└─ README.md
```

---

## Data location

```
%APPDATA%\Rebuffer\
├─ rebuffer.db          # metadata + search index
├─ settings.json
└─ blobs\
   ├─ ab\cd\abcd1234…   # content-addressed originals
   └─ thumbs\           # WebP previews
```

The store folder is configurable in Settings; changing it migrates existing data.

⚠️ **Your clipboard history is sensitive.** It sits in your user profile, protected by Windows file permissions, but it is not encrypted by default. Don't sync this folder to a shared drive.

---

## License

MIT
