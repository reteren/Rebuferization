# Rebuffer UI Performance Report — Grid at 10,000 Items (W26)

First look at the popup grid at scale. The store side is covered by `PERF.md`
(10,000 items, 200-item page read ≈ 924 µs). This document is the UI side:
does the hand-rolled virtualized grid (spacer + absolutely positioned cards
+ sticky date-group headers, `src/lib/components/Grid.svelte`) actually
scroll at 60 fps with 10,000 items in the store, as SPEC §10 asks?

**Verdict up front: the 60 fps question is moot until two rendering bugs are
fixed — the grid cannot display items past the first group boundary.**
Scroll *work* itself is fast (median 5.5–7 ms/frame, 60 fps achieved during
slow scrolling), but at any scroll depth past the first screen the cards are
positioned ~one extra group offset below where they belong and the sticky
headers stop sticking, so the grid progressively empties and is a blank
canvas at the bottom of 10,000 items. The SPEC §10 target is not met —
not because of raw speed, but because the rendering is wrong at scale.

## Test Environment

- **Machine**: AMD Ryzen 7 7800X3D (8C/16T), 32 GB RAM, NVMe SSD
- **OS**: Windows 11 Pro (build 26200), fully elevated session (`EnableLUA=0`,
  every process runs at High integrity — this matters, see CDP note)
- **WebView2 runtime**: **149.0.4022.98, pinned machine-wide via HKLM policy**
  (see *Method* — the installed 151.0.4129.107 cannot be debugged from this
  session)
- **Build**: `src-tauri\target\debug\rebuffer.exe` (dev profile), frontend via
  Vite dev server (`localhost:1420`), WebView2 window 1116×709, grid viewport
  ≈ 523×1,070 px (8 columns at zoom 3, 5 at zoom 5)
- **Store**: `%APPDATA%\Rebuffer`, seeded by `scripts\ui\seed_perf.ps1`:
  10,000 items — 5,600 plain text (short/medium/multi-paragraph), 1,200 code,
  1,400 links, 800 colours, 500 images (real PNG blobs + thumbnails that
  decode correctly, `naturalWidth` verified), 500 files with `item_files`,
  50 pinned; ages 0–29.5 days across 30 date groups, every group mixes kinds;
  every content hash is a real BLAKE3 via the existing `scripts\seed-tool`.
- Store count was re-checked with `sqlite3` immediately before and after the
  run: **10,000 both times** (the settings worker's own activity reset it
  several times earlier in the session; the numbers below were captured on a
  stable, freshly seeded store).

## Method

Live CDP (Chrome DevTools Protocol) against the WebView2 page, driven by
`scripts\ui\measure.mjs`; `scripts\ui\run_measured.ps1` orchestrates
(kill → launch → show popup via the real Alt+V hotkey → measure → stop).
The popup must be visible (rAF is throttled for hidden WebViews) — verified
`document.visibilityState === 'visible'` throughout.

**Frame timing** is the renderer's achieved frame cadence: a
`requestAnimationFrame` loop advances the grid's `scrollTop` and records
timestamps; frame time = delta between consecutive rAF callbacks, which
brackets the previous frame's script + layout + paint. Longtask entries
(`>50 ms` main-thread stalls) are collected alongside. This is real
main-thread frame timing; it does not include compositor vsync pacing or
real wheel-input smooth scrolling (the drive is programmatic scrollTop). All
scroll drives ran 3 down-and-back cycles except the deep runs.

**CDP availability note (why the runtime is pinned).** The session is fully
elevated (no filtered tokens exist), and WebView2 runtime 150+ refuses to
honor `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port` from
elevated hosts (Microsoft WebView2Feedback #5640/#5645). The OS-integrated
runtime also ignored the `BrowserExecutableFolder` *value*; the documented
policy *structure* (a `BrowserExecutableFolder` **subkey** whose value name
is the app name) pins `C:\WebView2Runtime\149.0.4022.98` (fixed-version 149,
where remote debugging works regardless of elevation), plus
`AdditionalBrowserArguments=--remote-debugging-port=9333`. The app is
launched through a `schtasks` task (`rbf-cdp`); the second-instance
single-instance-plugin trick used to show the popup fail-fasts (its own
WebView2 browser cannot bind the CDP port the first instance holds,
`0xc0000409`), so the popup is shown with the app's own Alt+V hotkey.

Raw results: `scripts\ui\out\results.jsonl` + screenshots in the same folder.

## Results

### 1. Scroll frame timing

| Drive | Frames | Median | p95 | p99 | Worst | >16.7 ms |
|---|---|---|---|---|---|---|
| slow, 150 px/frame, 3 cycles (list 200→~1,000 items as pages load) | 80 | **7.0 ms** | 13.2 | 15.6 | 15.6 | **0 (0 %)** |
| fast, 1,200 px/frame, up over 15,952 px | 15 | 14.4 ms | 21.2 | 21.2 | 21.2 | 3 (20 %) |
| **deep, 400 px/frame, full 10,000-item range** (21.6 s, 50 page loads) | 1,389 | **5.5 ms** | 8.3 | 345 | 539 | 56 (4 %) |
| deep fast, 1,200 px/frame, full range | 439 | 7.8 ms | 13.5 | 16.9 | 205 | 5 (1.1 %) |

Longtasks during the full-range deep scroll: **34 stalls of 50–439 ms**,
steadily worsening (50→439 ms) across the run — the loadMore page append +
list re-sort + re-render work per page. The p95 is excellent and the p99 is
killed by these paging stalls: a user flick-scrolling the whole list
experiences ~4 % of frames as multi-100 ms freezes.

At slow, deliberate scroll speeds the main thread holds 60 fps (0 frames
over budget). At flick speeds (2+ viewports/frame) it misses (worst 21 ms
without paging stalls; 205 ms with them).

### 2. Group headers under recycling — **BUG (confirmed)**

`position: sticky` headers are wrong for every group but the first once the
grid holds more than one screen of data:

| scrollTop | Header | header rect | group rect | expected | actual |
|---|---|---|---|---|---|
| 90,039 | 2 August | top 91, bottom 123 | top −2,231, bottom 123 | stuck at viewport top 0 | **at the group's bottom edge** |
| 90,039 | 1 August | top 2,901 | top 2,487, bottom 2,933 | stuck at 0 | **at the group's bottom edge** |
| 157,077 | 28 August | top 10,515 | (group starts below viewport) | natural | 10,515 (see note) |

Root cause: `.group` sections are positioned with the CSS `translate`
property (`translate:0 {sec.top}px`), and `position: sticky` computes its
constraint against the group's **layout** box (the in-flow position ≈ 0 for
the first group, then successive pre-transform flow positions), not its
transformed box. The first group ("Today", translate ≈ 16 px) sticks
correctly — repeatedly observed pinned at the viewport top. Every group
after the first is pushed to the *bottom edge* of its own box as soon as its
layout box scrolls past the viewport, so a header that should be pinned at
the top of the screen sits at the group's bottom — 91 px and 2,901 px below
the viewport top in the table above, and 6,584 px / 10,515 px below in other
snapshots. No duplicate or mislabelled headers were observed; the labels are
correct, the position is wrong.

### 3. Card virtualization — **BUG (confirmed, the headline one)**

The virtualized cards are double-offset by their group's top. In
`Grid.svelte`, `sections` computes each card's y as
`g.top + headerH + row * pitchH` — a **document** coordinate — but the card
is rendered inside the `.group` element, which is itself translated by
`g.top`, so the card lands at ≈ `2 × g.top`. The error is invisible at the
top of the grid (first group's top ≈ 16 px) — which is why it "looks fine
with 40 items" — and grows linearly with depth:

- At scrollTop 90,039 (zoom 1): 85 cards rendered, **0 in the viewport**;
  card rect tops at 87,435 while the group's document top is 87,808 (cards
  ~89,700 px below the viewport).
- At scrollTop 523,589 (the bottom of the full list): **0 cards, 0 headers
  rendered** — the grid is blank.
- The spacer keeps the correct total height (524,112 px at zoom 5 for all
  10,000 items), but the misplaced cards extend the scrollable overflow to
  779,190 px, so the scrollbar no longer matches the content and the visible
  range silently ends with an empty canvas.

### 4. Zoom at scale

ctrl+wheel 3→5 (2 effective steps): relayout settled in **547 ms**, frame
median 5.6 ms during the relayout, worst 18.7 ms, 1 frame over budget.
Zooming back down re-laid out cleanly. Responsiveness is fine; the headers
and cards are governed by the two bugs above at every zoom (the card
double-offset scales with the group tops, so deeper = emptier at any zoom).

### 5. Memory

| Phase | JS heap (used) | DOM cards | DOM nodes (CDP) |
|---|---|---|---|
| top, 200 items loaded | 6.9 MB | 40 | 1,853 |
| after slow+fast scroll (~1,000 items) | 8.8 MB | 64 | 2,251 |
| after full-range deep scroll (10,000 items) | 47–53 MB | 0 (empty viewport) | 35,385 |
| back at top after deep scroll | 29.5 MB | 96 | 5,528 |

The JS heap grows with the loaded list (10,000 `ItemDto`s ≈ 50 MB — the
legitimate cost of paging everything in, not a per-row leak), and the DOM
node count spiked during the deep scroll (35k) and **fell back** once the
recycled rows scrolled out of view (5.5k) — no monotonic growth, so no
evidence of a leaked-rows leak. `jsEventListeners` stayed at 32 throughout.
(The honest memory caveat: `Performance.memory`/`Memory.getDOMCounters` are
the renderer's own counters; a real leak check over many scroll cycles
wasn't run because the deep scroll already broke the viewport.)

### 6. Search and tabs at scale

- Typing `clipboard` (9 chars, 70 ms apart — inside the 120 ms debounce):
  keystroke handling 0.1–0.5 ms, frame median 5.6 ms during typing, **0
  frames over budget**; first grid mutation **57.7 ms** after the last
  keystroke (debounce + FTS IPC + render). Search is smooth at 10,000.
- Clearing the search: first grid mutation 127.4 ms.
- Tabs (Images 500 / Text 9,000 / Links 1,400 / Files 500 / Pinned 50):
  first grid mutation **127–158 ms** per tab, cards rendered, grid rebuilt
  smoothly. No tab exceeded 160 ms.

## Where the time goes

The scroll loop itself is cheap: the virtualization math (`groups`,
`sections`, `rowOf`) is a few milliseconds even over the full list, and the
visible-window DOM patch is small. The expensive frames are the **loadMore
page appends** (list concat + re-sort + re-derive + patch ≈ 50→439 ms each,
worsening as the list grows) and the **flick-speed layout** (~21 ms worst).
The dominant *user-visible* cost at 10,000 items, though, is neither: it's
that the grid stops showing anything — the two positioning bugs above, not
frame time, are what fail SPEC §10.

## What could not be tested

- Real wheel input with the compositor's smooth-scrolling (the driver sets
  `scrollTop` programmatically per frame — true to main-thread cost, not to
  input pipeline feel). Note that loadMore is confirmed working from
  programmatic scrolls: the spacer grew page by page during every scroll
  drive (4,164 → 20,564 px in the first test, and the deep run loaded all
  10,000 items — the spacer measured 524,112 px at zoom 5 afterwards).
- The 151 runtime — this session's elevation forces the 149 pin; frame
  numbers on 151 could differ.
- A genuine multi-cycle leak check (the deep scroll broke the viewport
  before a second cycle was meaningful).

## Repro

```powershell
# One-time (documented in the report): pin WebView2 149 + CDP port via HKLM,
# create the rbf-cdp scheduled task, seed the store.
scripts\ui\seed_perf.ps1
scripts\ui\run_measured.ps1            # kill -> launch -> hotkey popup -> measure -> stop
node scripts\ui\measure.mjs deep-down 400   # full-range scroll + frame stats
node scripts\ui\measure.mjs headers         # header/group geometry at current position
```

System changes this run introduced (all reversible): HKLM
`Software\Policies\Microsoft\Edge\WebView2\BrowserExecutableFolder` (`*` and
`rebuffer.exe` → `C:\WebView2Runtime\149.0.4022.98`) and
`AdditionalBrowserArguments` (`--remote-debugging-port=9333`); WER LocalDumps
for `rebuffer.exe`; scheduled task `rbf-cdp`; `settings.json` `window.zoomStep`
was left at 1 by the zoom test (the settings worker's scripts restore their
own snapshot).