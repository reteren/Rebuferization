# W35 — Verification of the day's fixes against the running app

Date: 31 Aug 2026. Machine: DESKTOP-0MFACBN. Binary: **release** (`src-tauri\target\release\rebuffer.exe`, built 23:47 after the last source change at 23:06), launched via the CDP-capable path (scheduled task `rbf-cdp` → WebView2 149 + `--remote-debugging-port=9333`). Store: ~10,017 items. Scripts under `scripts/verify/`; the app-lock protocol (`scripts/APP-LOCK.md`) was honored (lock taken, no contention, released at the end).

Verdict per item: **yes / no / could-not-test** with evidence.

## 1. The grid at depth — YES

The headline fix holds. `scripts/verify/grid_check.mjs` scrolled the full ~10,000-item range and sampled geometry at the bottom and at 25/50/75 % depth:

- Cards render at **every** depth: 43–55 cards in the viewport at each probe; at the very bottom (scrollTop 98,470 / scrollHeight 98,830) 43 cards are on screen. The old "blank canvas at the bottom" is gone.
- The pinned header is correct at each point: **1 August** at the bottom (matches the store's oldest group, verified against `MIN(created_at)`), **Yesterday** at 25 %, **21 August** at 50 %, **11 August** at 75 %. Exactly one header is pinned at a time and it sits at the grid's top edge (rectTop == grid box top) while the next group's header sits at its natural position.
- After a zoom change (tiles 72→116 px via ctrl+wheel, which re-lays out every tile) cards still render (22 in view) and the pinned header is still correct (**21 August** at the 50 % depth, its natural top re-derived under the new zoom).

## 2. More than 200 items — YES

`scripts/verify/loadmore_check.mjs`:

- Loading past 200: a fast burst grew the grid from 2,078 px (the initial 200-item window) and a full slow scroll loaded everything to a final 98,830 px; 10,006 distinct item ids were sampled across the range.
- No double-fire: during a 60-frame fast burst the MutationObserver counted only **2** page-appends (the `loadingMore` guard holds); no duplicate item id ever appeared in a viewport snapshot.
- Stops cleanly: scrollTop reaches the bottom, the spacer stops growing, and the bottom snapshot has no duplicate ids.

## 3. Selection — YES

`scripts/verify/selection_check.mjs` + `scripts/verify/close_check.ps1`, all against the real window:

- Ctrl+click toggles a card on/off; multiple ctrl+clicks accumulate. PASS.
- Shift+click extends a range from the anchor (3 cards selected). PASS.
- Ctrl+A selects all 66 in-view cards. PASS.
- Plain click with an empty selection copies and closes: the clipboard ended up **byte-identical to the clicked item's full blob text** (1,406 chars, exact match) and the window hid. PASS.
- Enter copies the focused item and closes (clipboard populated, window hid). PASS.
- Esc closes. PASS.

## 4. Drag-out — NO (does not complete a drop)

The path is wired (cards have `draggable` + `data-id`; the viewport's `ondragstart` → `beginDrag`), and a physical mouse drag **does** fire `dragstart` in the page (confirmed by an injected listener). `begin_drag` runs `DoDragDrop` on a dedicated thread — the log shows the drag thread executing. **But every single attempt ended with `drag finished, effect: 0` (DROPEFFECT_NONE)** — nothing landed in an Explorer window or on the desktop (both drop targets verified to be under the cursor), and no blob was ever materialized to a temp file. Several attempts additionally logged `drag ended with an error: 0x800401F0` (**CO_E_NOTINITIALIZED**) — the drag thread initializes COM with `CoInitializeEx(COINIT_APARTMENTTHREADED)` only, no `OleInitialize`, which is the usual requirement for `DoDragDrop`. This is reported as **not working**; the wiring and the loop execute, but no drag ever completes a drop. A human mouse operator should re-confirm, but the evidence (every drop NONE + the COM-init warning) points at a real defect in the drag path rather than pure automation failure.

## 5. Animated GIFs — YES

Seeded a real 2-frame animated GIF (`scripts` ffmpeg-built) as item 21001 with a static WebP thumb. Via CDP, the card's `<img>`:

- with `appearance.animateGifs = true`: src is `http://asset.localhost/…blobs/0f/73/0f737b…` — the **animated original blob** (an animated `img` plays it);
- with `appearance.animateGifs = false` (settings.json + restart): src is `…blobs/thumbs/0f737b….webp` — the **static frame thumb**.

The `animateGifs` switch reaches the card and selects the correct URL both ways.

## 6. The icon — partially verified (could-not-test the rendered pixels)

- The artwork is present and wired: `src-tauri/icons/` holds the full set (`icon.ico`, `32x32.png`, `128x128.png`, `source.png`, Square logos), `tauri.conf.json` `bundle.icon` references it, and the tray uses `include_bytes!("../icons/32x32.png")`. Not the Tauri default.
- **Could not test** the rendered tray/taskbar pixels: the notification-area toolbar was not enumerable from this session (UIA and `TB_GETBUTTONCOUNT` found nothing), and the app's windows are frameless + `skipTaskbar`, so there is no title-bar or taskbar button to inspect while hidden. The 32x32 artwork is downscaled to tray size with nearest-neighbour (per the icon commit), which is consistent with "sharp but noisy at 16–20 px" — but I cannot honestly confirm how it reads on screen.

## 7. Cold start — YES (against the 1.5 s target)

`scripts/verify/coldstart.ps1`, launch via the task on the 10,000-item store:

- launch → `rebuffer starting` (setup log): 454 ms
- setup → `hotkey Alt+V registered`: 678 ms
- **launch → hotkey-ready: 1,132 ms**
- The hotkey then worked immediately (Alt+V showed the popup).

The sweep rework (`f38f7b7`, "151 ms") is the bulk of that 678 ms setup tail — end-to-end it is ~1.1 s, comfortably under the 1.5 s target. The claimed 151 ms is the sweep portion only and is not separately observable from outside. One earlier launch measured ~19 s for the setup tail (first run after a cold WebView2 + freshly checkpointed WAL); it did not reproduce on three subsequent runs and looks environmental. The tray icon itself was not independently confirmed (UIA found no "Rebuffer" tray element); readiness was taken from the app's own log line plus the hotkey actually working.

## 8. The thumbnail fallback — YES

Deleted a visible image card's (20018) thumb file behind the app's back and restarted:

- The startup sweep cleared the dangling `thumb_path` (row now has `thumb_path = NULL`), the **item survived** (still present in the DB), and the card renders the **glyph-fallback placeholder** — `hasImg: false`, no broken-image icon (`complete && naturalWidth === 0` would have been true otherwise). PASS.

## What could not be tested

- The drag-out drop (item 4) completing into Explorer/desktop — a human mouse operator is needed; the evidence suggests a real defect (see item 4).
- The rendered tray icon and taskbar button pixels (item 6) — not enumerable from this session.
- Real wheel-input feel and long-run leak behaviour were not part of this pass.

## Repro / artifacts

- `scripts/verify/grid_check.mjs`, `loadmore_check.mjs`, `selection_check.mjs`, `act_once.mjs`, `eval_once.mjs`, `coldstart.ps1`, `close_check.ps1`
- Live evidence in `%APPDATA%\Rebuffer\logs\rebuffer.log.2026-08-31` (drag `effect: 0` / `0x800401F0` lines).