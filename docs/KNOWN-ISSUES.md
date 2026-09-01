# Known issues and untested ground

Written from the verification runs done during development. Nothing here is a
guess: each entry either has a reproduction or says plainly that it was never
exercised.

## Open defects

**The store stays wedged after its volume disconnects and returns.** With the
store on a removable or network path, unplugging it mid-session leaves the
process holding a dead SQLite connection: reads come back empty and writes
error until the app restarts. Reconnecting the volume does not recover it.
The app does not crash or hang, and it now starts normally when the volume is
absent at launch (it falls back to `%APPDATA%\Rebuffer` and says so), but a
mid-session disconnect needs a restart. Fixing it means detecting `SQLITE_IOERR`
on a store operation and reopening.

**A disconnected store reads as an empty one.** `list` returns `Ok([])` rather
than an error while the volume is gone, so the popup shows its empty state
instead of saying the store is unreachable.

**The janitor swallows its own errors.** `let _ =` in `store/mod.rs` means a
failed retention pass leaves no trace beyond the log.

## Verified only by construction

**Drag-out.** The OLE path is built and reachable — the card carries
`draggable` and `data-id`, the grid delegates `dragstart`, and `DoDragDrop`
runs on its own thread with OLE initialized. A drop was never completed by a
human with a mouse. An automated attempt failed for a reason since fixed (the
thread called `CoInitializeEx` where `DoDragDrop` needs `OleInitialize`), so
the last recorded evidence predates the fix.

**The tray icon at tray size.** The artwork is correct and wired, but the
notification-area pixels could not be enumerated to confirm how the
nearest-neighbour downscale reads at 16-20 px on screen.

**The installer's Win+V checkbox.** Confirmed compiled into the installer, with
the registry path, the label and the uninstall prompt all present, and proven
by reading the generated script that a silent install cannot touch the setting.
Never ticked in a live install, because no disposable environment was available.

## Never run

- Multi-monitor and mixed-DPI placement on real hardware. The arithmetic is
  unit-tested for negative coordinates, mixed scale factors, vertical monitors
  and a left-edge taskbar, but it has only ever run on one screen.
- The Windows 10 backdrop fallback, on Windows 10. The code path exists and is
  selected by build number.
- Capture from RDP sessions, virtual machines and fullscreen games.
- A 24-hour run for memory growth.
