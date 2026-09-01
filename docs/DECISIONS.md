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

Method (reproducible): with the app running and the popup hidden, a C# probe records a high-resolution timestamp (`Stopwatch.GetTimestamp`), injects `Alt+V` via `keybd_event` (the same chord the physical hotkey uses; `RegisterHotKey` fires on injected input), then busy-polls `IsWindowVisible(hwnd)` on a separate thread until the flag flips. Delta = chord-sent → `WS_VISIBLE` set. The popup is toggled closed between samples with a second `Alt+V` so every sample starts from the same hidden state. 15 samples: best 2.151 ms, median 4.125 ms, mean 4.397 ms, worst 7.003 ms.

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

## W23: popup rendering verification (30 Aug 2026)

Verified against the fresh debug build (post-commit bd8087e, `cargo build` green,
96 tests pass) on DESKTOP-0MFACBN at 100 % DPI (window DPI = system DPI = 96).
Evidence: 1:1 `CopyFromScreen` captures with OCR and pixel scans
(`verify11.png`, `state2.png`, `sc_00..09.png`, `verify_popup.ps1`,
`verify_11.ps1`, `gap_scan.ps1`, `popup_inspect.ps1`).

**Renders correctly (visually confirmed):** tabs with live counts, the Today
group header, age badges (`14m`…), format labels (`TXT`/`JSON`/`URL`/`PNG`), the
link card with favicon + domain (`github.com`), the file card (`DECISIONS.md`),
PNG image cards with real thumbnails, the status bar, the empty state, the
search box, the Newest sort control.

**Text overflow — verdict: NOT real; the earlier bleed impression was a
downscaling artifact.** At 1:1 there is exactly zero bright (text-like) pixels in
the 10 px gaps between cards across the full grid; the longest card renders 7
text lines and the clamp ellipsis, all inside the panel (rightmost glyph 126 px
vs. 139 px panel content edge). One quirk: with `-webkit-line-clamp: 7` +
`white-space: pre-wrap`, Blink places the ellipsis on its own 8th line rather
than at the end of line 7 — cosmetic, not overflow.

**Image thumbnails — the `http://asset.localhost` fix works in the fresh
binary.** The stale binary's broken-icon state (captured earlier by the previous
worker in `thumbfix*.png`) is gone: image cards render photo-like content (one
card: 75 distinct colors, sd 51). Caveat: broken-image icons still appear when
the item's `thumb_path` file does not exist on disk (11+ items after the other
worker's import produced `File does not exist at path: …\blobs\thumbs\*.webp`
asset-protocol errors). The card has no `onerror` fallback and the startup sweep
only prunes rows with a missing *primary* blob, never a missing thumbnail.

**SPEC 6.3 / 6.5 behaviors:** Esc closes — YES; outside click closes — YES
(focus-loss dismissal); zoom dial / ctrl+wheel changes tile size — YES (measured
116 px → 72 px tiles); arrows move focus — YES (2×ArrowRight then Enter copied
the 3rd grid item); Enter copies the focused item and closes (closeOnCopy) — YES.

**The full variety matrix verified visually**, with a fresh app instance whose
frontend loaded freshly-seeded items): the hex-colour card renders the `#3D8BFD`
swatch (≈37 k pixels of the exact colour filling the preview area), the code card
renders the JSON preview in mono (`"schema":`, `"version": 1`, `"items":`…), and
the link card renders the favicon (green gradient square) plus the 16m/17m/18m
age badges. Earlier attempts were sabotaged by concurrent verifiers fighting over
the single app instance; under the lock, a single restart + capture produced everything.

**Other findings recorded for the owners:**
1. `janitor::startup_sweep` is O(blob files) with a `COUNT(*)` per file —
   tray-ready degraded 1.05 s → 9.5 s → 53 s as the store grew to 7 k/10 k items
   (details in `docs/PERF.md`).
2. Several concurrent `rebuffer.exe` instances were observed running at once
   (three at one point) despite the single-instance plugin — rapid restart races
   can slip past the guard .
3. Intermittently the popup opened and dismissed itself within ~20 ms (focus-loss
   race on foreground handoff; reproduced once, see latency section above).
4. CDP is unreachable with the shipped config (wry overrides the WebView2
   additional-browser-arguments env var) — see the latency section.
5. Tooling notes: on this machine the keyboard layout is non-Latin, so keystroke
   injection must use `SendInput` + `KEYEVENTF_UNICODE` (VK codes silently type
   Cyrillic); PowerShell 7 static-method binding on `Add-Type` types rejects
   negative literals and coerced doubles — cast to `[int]` and prefer
   reflection invocation.

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