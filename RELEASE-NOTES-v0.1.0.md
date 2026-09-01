# Rebuffer v0.1.0

A fast, persistent clipboard history for Windows. Everything you copy stays for
30 days: text, images, GIFs, videos and files, all back with `Alt+V`.

## Install

Download **`Rebuffer_0.1.0_x64-setup.exe`** below and run it.

> **SmartScreen will warn on first run.** The build is not code-signed. Choose
> *More info → Run anyway*, or build from source, with the steps in the README.

The installer offers one option, unticked by default: disable Windows' own
`Win+V` history so the two do not compete. It writes a single per-user registry
value, and the uninstaller offers to put it back, but only when the installer
was the one that changed it. The same switch is in Settings → General.

## What it does

- Captures text, rich text, images, animated GIFs, video and file lists, with
  the source application recorded
- A privacy filter that fails **closed**: password managers are blocked by name,
  apps that mark their clipboard as excluded are honoured, and a source that
  cannot be identified is skipped rather than stored
- `Alt+V` opens a card grid centred on the cursor, in under 5 ms
- Search, sort, type filters, pinning, multi-select, drag-out into other apps
- A shelf: `+ Add` keeps files by reference, never copied and never expired
- Eleven themes, each with its own accent, chosen from live previews
- 30-day retention with an optional size cap, export and import

## Measured

On a Ryzen 7 7800X3D running Windows 11:

| | measured | target |
|---|---|---|
| Hotkey → window visible | 4.1 ms median, 7.0 ms worst | < 80 ms |
| Idle RAM | 45.8 MB | < 60 MB |
| Idle CPU | 0 % | 0 % |
| Cold start, 10,000 items | 1.13 s | < 1.5 s |
| A 200-item page at 10,000 items | ≈ 0.9 ms | < 10 ms |

## Known issues

Listed in [`docs/KNOWN-ISSUES.md`](docs/KNOWN-ISSUES.md), including what was
verified only by reading rather than by running. The two worth knowing before
you install:

- A store on a removable or network volume stays unusable until restart if the
  volume disconnects mid-session. It starts fine when the volume is simply
  absent, falling back to the default location.
- Drag-out is built and reachable but was never completed by a human with a
  mouse.

Multi-monitor placement, the Windows 10 backdrop fallback, capture from RDP and
virtual machines, and long-run memory behaviour have not been tested on real
hardware.

## License

MIT.
