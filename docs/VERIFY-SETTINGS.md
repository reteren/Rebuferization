# VERIFY-SETTINGS — W24 acceptance run

Date: 2026-08-30, ~18:20–19:35 UTC.
Target: `src-tauri\target\debug\rebuffer.exe` built from the current tree
(cargo build at 18:36, 96 tests green per the coordinator). The binary used
for the run was built with `TAURI_CONFIG` baking `--remote-debugging-port=9333`
into the two windows so the CDP harness (`scripts/settings/01–09`) can attach;
no source file was modified for this (env-var config merge only), and the
pristine binary was rebuilt afterwards.

Frontend was served by the live vite dev server (`localhost:1420`, running
since earlier today), so the DOM under test is the current source tree.

Scope note: the run happened while other workers were testing the same app and
store. The coordinator granted exclusive app ownership mid-run (scripts/.app-lock
convention); interference still occurred (see Findings F7) and one Phase-A
stress check had to be re-verified manually. Everything marked PASS was
re-confirmed on a session I owned.

Verdicts are per the brief: **yes / no / could-not-test**, with evidence.

---

## 1. Does it render? — YES

All six sections render with their full control sets. Evidence:
- `scripts/settings/shots/03-{general,storage,appearance,privacy,data,about}.png`
  (CDP page captures, one per section after clicking the nav button).
- `scripts/settings/shots/03-*-inventory.txt` — per-section control inventory
  (hotkey box + Record + 7 toggles in General; 4 number inputs + meter + Clean
  now in Storage; radios/sliders/select/color in Appearance; blocklist editor in
  Privacy; export/import/clear/reset in Data; About).
- The settings window is natively visible after the gear path
  (`IsWindowVisible`=True, measured via enum-windows.ps1 at 18:45).
- Route check: real Alt+V → popup → gear button → settings window showed
  (`02-open-settings.ps1`).

## 2. Does each control WRITE? — YES (44/44)

`04-write-tests.ps1` drove every control through the real DOM and diffed
`%APPDATA%\Rebuffer\settings.json` afterwards. Journal:
`scripts/settings/shots/04-write-journal.txt` — 44/44 PASS. Covered: hotkey
record/restore (Alt+Shift+Z → Alt+V), aggressiveMode, launchOnStartup,
silentStart, captureEnabled, autoPaste, pasteAsPlainText, closeOnCopy,
retentionDays, maxItemBytes, maxStoreBytes, notifyWhenFull, sizeMode,
fixed.width, percentOfMonitor, zoomStep, showAge, formatLabelSize,
animateGifs, reduceMotion, accent, respectClipboardFlags, blockedProcesses
add + remove. Each control is bound to a real settings field; none is
decorative.

## 3. Does each change TAKE EFFECT?

| Setting | Verdict | Evidence |
|---|---|---|
| Hotkey re-registration | **YES (functional), NO (log)** | Real press of the new chord (Ctrl+Alt+Shift+Z) opened the popup: log `hotkey fired` + popup visibilityState visible (05 E1). BUT the log line `hotkey ... registered` appears only at startup — `rebind` never logs re-registrations (finding F2). |
| captureEnabled stops/restores capture | **YES** | Off: a clipboard copy landed 0 rows in rebuffer.db. On: the same copy landed a row (05 E2, rows verified via sqlite3). |
| retentionDays → janitor | **YES** | Item aged 40 days removed by the policy janitor with retentionDays=30 (`{"removedItems":1}` — 05b R1). |
| maxStoreBytes → janitor | **YES** | Cap 1 MB set, 2 MB text item captured, policy janitor pruned it (05b R3, removedItems 1946 → under cap). |
| accent recolours the popup | **NO** | settings.json wrote #ff00ff, but the popup's computed `--accent` stayed #7aa2ff and `.add-btn` background stayed rgb(122,162,255) (05 E3). Root cause: nothing in src/ ever assigns the `--accent` CSS variable from settings — `tokens.css` even comments "accent is overridden at runtime from settings", but no code does it (grep for setProperty/documentElement confirmed). Genuine dead setting (finding F1). |
| launchOnStartup ↔ HKCU Run | **YES** | Run key removed on off, re-added on on (05 E4, registry read both ways). |
| window size on the real popup | **YES** | `GetWindowRect` on the popup HWND: percent/40 → 1024x557 (exactly 40 % of the 2560x1392 work area under the cursor); fixed 760x520 → 760x520 exactly. (06 C — see notes in the 06 journal; the printed FAIL was leftover-state pollution, the pair of measurements above is the verdict.) |
| zoomStep live | **could-not-test (one-time read)** | Popup.svelte reads `settings.current.window.zoomStep` once at mount; no live listener for it. It applies to the popup on its next load/restart, not while running. |
| external edit hot-reload | **YES backend / NO UI** | Hand-edited retentionDays 12 → `get_settings` returns 12 within ~4 s (06 D PASS). The settings window's inputs keep showing the old value (no settings-changed event; the store only re-syncs on a patch or restart). |

## 4. Hotkey capture field — YES

- Records the next chord: Alt+Shift+Z recorded (04 G1), and the re-registration
  works (03 item above).
- Reserved chord rejected with an inline explanation: pressing Win+V while
  recording shows "Win+V is Windows' clipboard history — Windows owns that chord
  and RegisterHotKey will refuse it. Switch on Aggressive mode to claim it
  anyway." settings.json unchanged (04 G1b).
- Bare-key warning: single `P` records (the field does record it) and shows
  "A bare-key hotkey fires while you type — hold a modifier." (04 G1c).
- Unusable keys rejected: Tab → "That key cannot be used as a hotkey. Use a
  letter, digit, or F1–F24." (04 G1d).
- **Backend rollback — confirmed exactly as the brief predicts** (06 A):
  invoking `update_settings` directly with `{"patch":{"hotkey":{"binding":"Win+V"}}}`:
  - returns `INVOKE_ERR:"RegisterHotKey refused this combination (the settings UI explains reserved chords)"`;
  - settings.json still shows `Alt+V` (old binding kept);
  - the UI hotkey box still shows Alt+V;
  - the log records the rollback: `rebind to Win+V (aggressive: false) failed:
    RegisterHotKey refused this combination ... restoring the previous binding`.
  So the chord is validated before persist and the hotkey section is rolled back
  on failure — observed on a live instance.

## 5. Storage meter — YES (real numbers), with a staleness caveat

- Legend numbers match the database: image kind `199 items · 59.3 KB` vs
  sqlite `image:199:60758` — byte-for-byte (05 E6).
- Meter head `Storage usage ... items` matches the legend sum.
- "Clean now" actually deletes: item aged 40 days removed with feedback
  "Removed 1 items · freed 20 B." (05b R2; also 05 E5 path — the first run's
  NO_MSG was a re-render race, re-proven on the second run).
- **Staleness (finding F3)**: the meter is refreshed only on settings-window
  mount, "Clean now", or import. The background janitor's deletions and new
  captures do not refresh it — measured: meter showed 1996 items / 2.43 MB
  while the DB had ~250 items after a direct janitor invoke.

## 6. Export and import — YES

`07-export-import.ps1` (command level — the buttons' commands; the native file
dialogs cannot be driven headlessly):
- Export produced `export.rbx` (zip): contains `settings.json`, `manifest.json`,
  `items.jsonl`, and `blobs/` + `blobs/thumbs/` (10500 entries).
- manifest `itemCount`=10000 == items.jsonl lines == DB rows (consistent).
- Import **merge**: row count unchanged (10000 → 10000) — existing hashes
  skipped — and `copy_count` bumped 1 → 2 for existing rows.
- Import **replace**: full history restored (10000), `PRAGMA integrity_check` = ok.

## 7. Survive the stress — YES

`08-stress.ps1` Phase A changed every setting in one session, restarted, and
compared settings.json before/after — the two blocks in the journal are
identical: hotkey Ctrl+Alt+Shift+K, retentionDays 5, sizeMode fixed, zoomStep 5,
accent #00ff88, blockedProcesses +stressapp.exe, etc. Item count and db
integrity stable across the restart. (The journal's single FAIL,
`window.fixed.width 900`, is a harness quoting bug — the eval's embedded quotes
were mangled by PowerShell→node argv parsing; the same write passes in 04 and a
manual fixed-size 1100x600 survived a restart. Not an app defect.)

Invalid hand-edits (re-verified manually because the stress run's own instance
was killed mid-Phase-B by another worker — 08b section of the journal):
- **B1** retentionDays 999, accent "not-a-colour", formatLabelSize "huge",
  zoomStep 99, percentOfMonitor 999 → app starts (CDP up, hotkey registered);
  `get_settings` returns 30 / #7aa2ff / medium / 5 / 100 — clamped, not refused.
- **B2** malformed JSON tail (`} } not-json-tail`) → app starts; `get_settings`
  returns the full defaults (maxItemBytes 268435456, sizeMode percent,
  percentOfMonitor 40, zoomStep 3). See finding F4 for the missing log line.

---

## Findings

**F1 — Accent colour is a dead setting (P1).** It writes to settings.json and
the settings UI, but nothing applies it: no code assigns `--accent` from
`appearance.accent`. The popup (and settings window) always render the
tokens.css fallback #7aa2ff. The comment in `src/lib/styles/tokens.css` that
claims a runtime override is false. (05 E3.)

> **Resolution (W31):** Fixed. `src/lib/stores/settings.svelte.ts` now applies
> `--accent` to `document.documentElement` on every settings load, patch, and
> live `settings-changed` event, in both windows (each WebView owns its own
> document). `--accent-soft`/`--accent-strong` follow automatically because
> they are `color-mix()`ed from `--accent`. While auditing the other appearance
> fields: `showAge` and `formatLabelSize` are live (plumbed to Grid/Card);
> `reduceMotion` was equally dead and is now wired — the store toggles
> `data-reduce-motion` on `<html>`, which the existing `global.css` override
> keys on; `animateGifs` is dead in production and the real fix is Rust-side:
> `write_thumbnail` (src-tauri/src/store/blobs.rs) always decodes the first
> frame to a static WebP, so no production preview ever animates and the toggle
> has nothing to control. See the W31 notes below.

**F2 — Hotkey re-registration is not logged (P3).** `hotkey <chord> registered`
is logged once at startup only; a live rebind through update_settings never
logs success or failure (the failure path logs only from `rebind`'s warn, and
the success path logs nothing at all). The brief's suggested evidence channel
("logs record 'hotkey ... registered'") only works for the startup
registration. (05 E1, 06 B.)

**F3 — Storage meter is a snapshot, not live (P2/P3).** Refreshed on
settings-window mount, "Clean now", and import only. Background janitor runs
(cap pruning, startup sweep) and new captures are not reflected, so the meter
can show 1996 items while the store holds ~250. (05 E6 + interactive re-check.)

> **Resolution (W31):** Fixed. `src/routes/Settings.svelte` now refreshes the
> meter when the Storage section is navigated to, and subscribes to
> `item-added`, `items-deleted`, and `storage-warning` for the life of the
> window so captures and janitor prunes update it live. "Clean now" already
> repaints from the `CleanupResult` it receives (removed/freed line) and then
> refetches stats; that path is unchanged.

**F4 — Corrupt-settings fallback is unobservable in the log (P3).** With a
malformed settings.json the app starts on defaults (verified), but the
"settings.json is not valid JSON, using defaults" warning never reaches the
log file: `SettingsStore::load` runs before `logging::init` in `lib.rs` setup,
so the tracing subscriber does not exist yet and the warning is dropped.
(lib.rs:68-71; B2 re-verification.)

**F5 — Aggressive-mode hook never fired in this environment (could-not-test vs
broken).** The backend accepts and persists `Win+V` with aggressiveMode=true,
and the hook thread reports installed (invoke succeeds), but real SendInput
presses of Win+V and Ctrl+Alt+Shift+Q produced no `hotkey fired` within 60 s
polls, while the standard RegisterHotKey path fired within seconds on the same
instance (06 B, manual re-runs). The UI copy "Switch on Aggressive mode to
claim it anyway" is therefore unverified live; a physical-keyboard check is
needed to separate "hook never fires" from "injected input never reaches the
hook".

**F6 — Popup thumbnails 404 after the janitor prunes (P3).** While a popup is
open, deleting items via the janitor leaves its rendered `<img>` pointing at
blobs that no longer exist (asset protocol errors in the log, 19:01:06Z). The
popup does not react to item deletion.

> **Resolution (W31):** Frontend fixed; one part routed to the store (Rust).
> `src/lib/components/Card.svelte` now (a) skips the thumbnail entirely for
> rows already marked `missing` — those render the same placeholder a
> thumbnail-less card uses — and (b) attaches an `onerror` fallback: a failed
> `<img>` (asset-protocol 404) is swapped for the glyph placeholder instead of
> a broken-image icon, and the element is removed so the card stops
> re-requesting the missing file. The remaining "stop asking" fix belongs to
> the store: `missing` is only computed for `is_reference` rows today, and a
> non-reference row whose `thumb_path` file is gone still serves a
> `thumbUrl` that 404s. The store should either null the thumb URL when the
> thumb file is absent at query time (or prune such rows), or serve a
> placeholder from the asset protocol. Routing that to you.

**F7 — Shared-store interference during the run (process note, not an app
defect).** Other workers' sessions launched/killed rebuffer repeatedly (also
changing settings.json, e.g. zoomStep 3→1) while the lock was fresh; the
coordinator confirmed this was cross-worker interference and told the workers
to stop. My journals mark which checks were affected; the affected checks were
re-verified on an owned session. The app lock convention (scripts/APP-LOCK.md)
was followed; I deleted the lock when done.

---

## W31 — Resolutions and routing notes

Scope of the W31 fix: `src/routes/Settings.svelte`, `src/routes/Popup.svelte`,
`src/lib/stores/**`, `src/lib/styles/**`, `src/lib/components/Card.svelte`.
`Grid.svelte` and all Rust were left untouched. Verified with `npx tsc
--noEmit` and `npx svelte-check` — both at zero errors and zero warnings.
Per the shared-instance rule (scripts/APP-LOCK.md) the app was NOT launched, so
everything below is confirmed by source, type-check, and the W24 evidence
rather than a live run.

**Task 1 — accent + appearance audit.**
- `--accent` now assigned on the document root from the settings store, applied
  on initial load, on every `patch`, on `reload`, and on every live
  `settings-changed` event — in both windows.
- `reduceMotion` was dead (the `data-reduce-motion` CSS existed but nothing set
  the attribute); the same store effect now toggles it.
- `showAge` and `formatLabelSize`: confirmed live (plumbed to Grid → Card).
- `animateGifs`: **routed elsewhere (Rust).** Production thumbnails are always
  static — `write_thumbnail` in src-tauri/src/store/blobs.rs decodes the first
  frame to a WebP — so no preview ever animates and the toggle has nothing to
  control. A frontend-only wiring would require per-card `get_item_blob_url`
  invokes in a leaf component (violating the "components never call invoke"
  invariant) for an animated source that cannot be produced frontend-side. The
  fix belongs in the store: honour `animate_gifs` when generating thumbs (emit
  an animated WebP/GIF when enabled), or drop the field. Not changed here.

**Task 2 — thumbnail 404 fallback.** See F6 resolution above. Frontend fallback
done; the "stop asking" store-side fix (null the thumb URL / prune rows whose
thumb file is missing, or serve a placeholder) is routed to the Rust owner.

**Task 3 — storage meter.** See F3 resolution above. Event-driven refresh +
refresh on section show; "Clean now" already paints from its `CleanupResult`.

**Task 4 — progress banner + error surfacing.**
- Confirmed the banner clears on completion: the `store-progress` handler
  nulls it on `phase === 'completed'`, and the backend (janitor.rs) always
  emits a trailing `completed` for relocate/export/import. Since a *failed*
  operation may emit no `completed`, `pickStoreLocation`/`doExport`/`doImport`
  now also clear the banner in `finally` so it can never stick on an error path.
- Error surfacing confirmed present for relocate/export/import (each already
  caught and set the banner); the messages now carry operation context
  ("Export failed: …", "Import failed: …", "Could not move the store: …",
  "Clean up failed: …") so the user learns which action failed, not just that
  one did.

**Could not confirm without running the app:** the live recolour of both
windows by the accent picker, the thumbnail fallback rendering, the meter
repainting on events, and the progress-banner clear-on-failure — all are
type-checked and follow directly from the W24 evidence, but none was exercised
against a running binary per the lock rule.

## What was NOT tested (could-not-test)

- **Aggressive-mode hook firing** — see F5 (needs a physical keyboard / an
  input context that reaches the hook).
- **Zoom-step live application** — one-time read at popup mount (see §3).
- **Store relocation ("Change…" button)** — moves the shared store; not part of
  the brief's acceptance list; requires a real directory dialog.
- **Clear-history / Reset-everything buttons** — destructive on the shared
  store; not part of the acceptance list.
- **The native file dialogs behind Export…/Import…** — exercised the commands
  instead (see §6).
- **Accent on the settings window itself** — same root cause as F1 (no runtime
  assignment exists), so both windows are affected.

## Harness fixes made during the run (all inside scripts/settings/)

- `cdp.mjs`: `shot` read `rest[2]` instead of `rest[1]` (screenshots never
  worked); invoke args can now be passed base64 (`b64:` prefix) because
  PowerShell→node argv parsing mangles raw JSON braces/quotes.
- `02/05`: popup target matcher `*index.html*` missed the dev-server URL `/`;
  `$pid` renamed to `$pupid` (PowerShell reserved variable).
- `04`: two evals with `aria-label=\"...\"` re-written without embedded double
  quotes (PowerShell argv mangling).
- `05b-reverify.ps1` (new): re-proved the janitor flows with a non-destructive
  policy after the first run raced a re-render storm.
- `06-hardened-tests.ps1` (new — the missing 06): backend refusal/rollback,
  aggressive-mode registration, real-window size application, external-edit
  hot reload.
- `08-stress.ps1`: fixed-width/height evals re-written (same quoting issue);
  08b notes appended to the journal documenting the manual B1/B2 re-checks.

## Evidence inventory (scripts/settings/shots/)

04-write-journal.txt (44/44), 05-effect-journal.txt, 05b-reverify-journal.txt,
06-hardened-journal.txt, 07-export-import-journal.txt, 08-stress-journal.txt
(+08b), 03-*.png + 03-*-inventory.txt, 06-popup-fixed-size.png,
08-stress-all-changed.png. Snapshot state (settings.json / Run key / log
position / test-item ids) under scripts/settings/snapshot/; test rows deleted
and settings.json + Run key restored by 09-restore.ps1.