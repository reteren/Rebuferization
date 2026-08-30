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

**Target:** < 80 ms. **Measured: median 4.1 ms, worst 7.0 ms over 15 samples — PASS.** Machine: DESKTOP-0MFACBN (AMD Ryzen 7 7800X3D, 16 logical cores), Windows 11, debug build.

Method (reproducible — `scripts/latency.ps1`): with the app running and the popup hidden, a C# probe records a high-resolution timestamp (`Stopwatch.GetTimestamp`), injects `Alt+V` via `keybd_event` (the same chord the physical hotkey uses; `RegisterHotKey` fires on injected input), then busy-polls `IsWindowVisible(hwnd)` on a separate thread until the flag flips. Delta = chord-sent → `WS_VISIBLE` set. The popup is toggled closed between samples with a second `Alt+V` so every sample starts from the same hidden state. 15 samples: best 2.151 ms, median 4.125 ms, mean 4.397 ms, worst 7.003 ms.

The path is: LL hook callback (atomics + `PostMessage`, no allocation — order of
microseconds) or `WM_HOTKEY` delivery → `show_popup` → `GetCursorPos` +
`GetMonitorInfoW` + `GetDpiForMonitor` + `SetWindowPos` + `ShowWindow` +
`SetFocus` — all cheap Win32 calls on a pre-created WebView2 window. The
300–600 ms WebView2 creation cost is avoided by design (windows are created
hidden in `tauri.conf.json`).

Two measurement caveats worth knowing:
- The measurement includes the OS-level keyboard injection latency itself, so the app-side number is even smaller than reported; the worst sample was 7 ms.
- Observed once: on a coldly-restarted instance the popup opened at t=1 ms and dismissed itself at t=20 ms (focus-loss dismissal racing a foreground handoff while the injection was still landing). It did not reproduce across ~25 further samples; if the user ever sees the popup "flash and vanish" right after a hotkey press, this race is the first place to look (the `Focused(false)` dismissal path in `window/mod.rs`).

Also note: CDP inspection of the popup page is not possible with this config —
wry 0.55 always calls `CoreWebView2EnvironmentOptions::set_additional_browser_arguments`,
which per WebView2 semantics overrides the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
environment variable, so `--remote-debugging-port` can never be enabled without
editing `tauri.conf.json`.

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