# Rebuffer — decisions log

## W3: window & hotkey

### SPEC §11 — popup activation: the popup ACTIVATES

**Chosen:** the popup takes focus when shown (plain `show` + `set_focus`), and the
previously focused window is restored on hide.

**Why:** the alternative (`WS_EX_NOACTIVATE`) means the WebView never receives
normal keyboard input, so arrows/typing/Esc would need a raw-input or hook
plumbing path to reach the page — fragile with WebView2 and easy to get wrong.
The activation approach is what Win+V itself does, and the coordinator-owned
`popup_ready` command already calls `set_focus()` on the popup, which assumes an
activatable window. The "focus flicker" downside is largely mitigated here:

- the foreground window is cached (`GetForegroundWindow`) *before* the popup
  shows, so it can be handed focus back on hide;
- the popup is `alwaysOnTop` and positions at the cursor, so it visually appears
  "attached" to where the user is typing.

**Cost accepted:** while the popup is open, the previous app is not focused
(its title bar dims). This matches Win+V and is what the spec's §5.2
"simplest workable approach" describes.

### Hotkey → visible latency

**Target:** < 80 ms. **Measured value: pending a runnable build.**

The path is: LL hook callback (atomics + `PostMessage`, no allocation — order of
microseconds) or `WM_HOTKEY` delivery → `show_popup` → `GetCursorPos` +
`GetMonitorInfoW` + `GetDpiForMonitor` + `SetWindowPos` + `ShowWindow` +
`SetFocus` — all cheap Win32 calls on a pre-created WebView2 window. The
300–600 ms WebView2 creation cost is avoided by design (windows are created
hidden in `tauri.conf.json`).

A wall-clock number cannot be recorded yet: the app panics in `setup` until W4's
`SettingsStore::load` lands (it is still a `todo!()`), so `cargo run` is not
possible this phase. When it runs, measure with an `Instant::now()` in the
hotkey callback vs. a `popup-ready` emit (or a `tracing` timestamps diff); the
architecture keeps the budget comfortably under 80 ms.

### Aggressive mode (Win+V) — known caveat

The `WH_KEYBOARD_LL` hook swallows the chord's keydown before the shell sees it,
as required ("nothing but compare the chord and `PostMessage`"). Because the
`Win` key itself is not swallowed (that would require replaying it when the
next key is not part of the chord — out of scope), pressing `Win+V` can briefly
flash the Start menu on some Windows builds before the popup appears. The
settings UI already warns about aggressive mode; this caveat should be listed
there too.

### Chord parsing / reserved list

`Chord::parse` accepts `Ctrl/Alt/Shift/Win` + letters, digits, `F1`–`F24`, and
named keys (`Space`, `Tab`, `Esc`, arrows, `PrtScn`, numpad…). `is_system_reserved`
covers the Win+letter/digit/F-key/navigation set, `Ctrl+Alt+Del`,
`Ctrl+Shift+Esc`, `Ctrl+Esc`, and `Alt+Tab/Esc/Space/F4` — the combinations
`RegisterHotKey` refuses. The UI should present these with the inline
explanation the settings spec describes.

### Rebind wiring (open seam)

`HotkeyManager::new` deliberately binds nothing (the contract passes no chord).
`rebind(&Chord, aggressive)` swaps the binding and path at runtime, but no
coordinator-owned file calls it yet: `lib.rs` setup creates the manager and
`commands.rs::update_settings` does not rebind. The coordinator has been asked
whether they wire two calls (setup: initial chord from settings; update:
rebind on change) or prefer W3 to self-register the default `Alt+V`.