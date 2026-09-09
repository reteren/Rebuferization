<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="140" alt="Rebuffer">
</p>

<h1 align="center">Rebuffer</h1>

A convenient utility that saves everything you copy: from links to GIFs and
videos. History is stored for 30 days by default, but this limit can be easily
increased in the settings.

The intuitive interface keeps everything visual: GIFs animate, photos display
previews, and long texts show brief descriptions. Every file is labeled by
format, while fast search and sorting help you find what you need instantly.

The window and UI scale smoothly. Right-clicking a file opens a management menu
where you can instantly open, save, or perform other actions.

Settings allow you to adjust history clearing intervals, limit maximum file
sizes, and clear the buffer manually. You also get 11 themes, data export, and
a privacy tab to block specific applications, preventing passwords and
sensitive data from being accidentally copied.

![The popup grid showing text, code, link, colour, image and file cards](docs/img/popup.png)

---

## Status

**Shipped:** clipboard capture (text, rich text, images, video, files), duplicate bumping, privacy-flag filtering and a per-app blocklist, the `Alt+V` popup, the virtualized card grid with day grouping and a zoom dial, tabs, search/sort/filter, pinning, keyboard navigation, click-to-copy and optional auto-paste, drag-out into other apps, the shelf (`+ Add` stores references), the right-click menu, retention janitor, storage cap, tray with Settings / Enable-Disable, silent autostart, the settings window, and export/import of history and settings.

**Measured and passing** (on DESKTOP-0MFACBN, an AMD Ryzen 7 7800X3D, Windows 11, debug build; recorded in `docs/PERF.md` and `docs/DECISIONS.md`):

| Metric | Measured | Target |
|---|---|---|
| Hotkey → window visible | **4.1 ms median** (worst 7.0 ms) | < 80 ms |
| Idle RAM, main process | **45.8 MB median** | < 60 MB |
| Idle CPU | **0 %** (max 0.36 % of one core) | 0 % |
| 200-item store page at 10,000 items | **≈ 0.9 ms** | < 10 ms |
| Cold start to tray-ready, 10,000-item store | **1.13 s** | < 1.5 s |
| Startup integrity sweep, 10,000 items | **151 ms** (release) | n/a |

**Known issues and untested ground** are listed honestly in
[`docs/KNOWN-ISSUES.md`](docs/KNOWN-ISSUES.md), including what was verified
only by reading rather than by running.

---

## Install

Grab `Rebuffer_x.y.z_x64-setup.exe` from the
[latest release](../../releases/latest) and run it.

The installer offers one option, unticked by default: **disable Windows'
own clipboard history (Win+V)**, so the two do not compete. It writes a
single per-user registry value and the uninstaller offers to put it back,
but only if the installer was the one that changed it. The same switch lives
in Settings → General, so you can change your mind later.
See [`docs/INSTALLER.md`](docs/INSTALLER.md).

> **SmartScreen will warn on first run.** The build is not code-signed:
> certificates cost money this project does not have. Choose *More info →
> Run anyway*, or build it yourself with the steps below.

---

## Features

**Capture**
- Records every clipboard change: Unicode text, rich text (HTML/RTF), images (PNG/JPG/GIF/WebP/BMP), videos, and file references
- Survives reboots, crashes, and force-kills. Nothing is buffered in memory waiting to be written
- Duplicate detection: copying the same thing again bumps the existing entry to the top instead of creating a clone
- Respects clipboard privacy flags, so password managers never end up in your history
- Per-application blocklist for anything else you don't want recorded

**Browse**
- Opens next to your cursor, clamped so the window always fits on the monitor you opened it on
- Grid of cards with a zoom dial you can drag or scroll (`Ctrl`+wheel also zooms)
- Grouped by day (Today, Yesterday, specific dates), like Explorer's date grouping
- Tabs: All / Images / Text / Links / Files / Pinned
- Search, sort (name, size, newest, oldest), and filter by type or exact extension
- Relative age badge on each card (`14m`, `3h`, `4d`)
- Format label on each card (`PNG`, `TXT`, `MP4`)
- Full keyboard navigation: arrows, Enter, Esc, Ctrl/Shift multi-select
- Pin anything to keep it past the auto-clean window

**Use**
- Click to copy back to the clipboard, or enable auto-paste to drop it straight into whatever you were typing in
- Drag items out into Explorer or any other app
- Add files manually with the **+** button. Those are stored by reference and never expire
- Right-click menu: Open, Open with, Save as, Show in folder, Rename, Pin, Delete

**Manage**
- Auto-clean after 1–30 days (configurable)
- Optional storage cap: when the store exceeds your limit, the oldest unpinned items are removed and you get a notification
- Tray icon with two entries: Settings, and Enable/Disable (left-click opens the popup)
- Silent autostart with Windows
- Export/import of history and settings

![The settings window](docs/img/settings.png)

---

**Appearance**

- Eleven themes: dark blue, black, grey, light, sky blue, dark green, dark
  purple, ember, ocean, wine and paper, each with its own accent, picked from
  a strip of live previews that paint themselves in the theme they name
- Any accent can be overridden per taste, or left to follow the theme
- Zoom dial, configurable card badges, reduced-motion honoured

---

## Getting started

### Prerequisites

1. **Rust**, installed via [rustup](https://rustup.rs/), stable toolchain, `x86_64-pc-windows-msvc`
2. **Visual Studio Build Tools 2022** with the *Desktop development with C++* workload (Tauri needs the MSVC linker)
3. **Node.js 20+** and npm
4. **WebView2 Runtime**, preinstalled on Windows 11; on Windows 10 grab the Evergreen Bootstrapper from Microsoft

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

Output lands in `src-tauri/target/release/bundle/nsis/`. The build is unsigned, so SmartScreen will warn on first run, which is expected for an unsigned open-source binary.

---

## License

MIT, see [`LICENSE`](LICENSE).
