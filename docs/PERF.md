# Rebuffer Store Performance Report (10,000 Items)

This document records the baseline persistence performance metrics for the Rebuffer SQLite store and content-addressed blob subsystem, fulfilling ROADMAP Phase 7 requirements.

## Test Environment

- **CPU**: AMD Ryzen 7 7800X3D (8 Cores, 16 Logical Processors)
- **Operating System**: Microsoft Windows 11 Enterprise (64-bit)
- **Rust Profile**: `dev` test build (`[unoptimized + debuginfo]`)
- **Storage Subsystem**: NVMe SSD / NTFS (temp directory via `tempfile`)
- **Dataset Size**: 10,000 items (mix of plain text, rich text, images, multi-file collections, and references)

## Runtime numbers (SPEC §10)

Measured on **DESKTOP-0MFACBN** (AMD Ryzen 7 7800X3D, 16 logical cores, Windows 11,
debug build), 30 Aug 2026, by the W23 perf worker. The app sat in the tray (popup
hidden) with a warm WebView2; scripts live in `scripts/` (`idle.ps1`, `coldstart.ps1`).

| Metric | Target | Measured | Verdict |
|---|---|---|---|
| Cold start to tray-ready | < 1.5 s | **~1.05 s** with a ~34-item store | PASS |
| Cold start to tray-ready, 7 043-item store | < 1.5 s | **~9.5 s** (old sweep) → **~1.3 s** with the W29 fix | MISS → PASS |
| Cold start to tray-ready, 10 000-item store | < 1.5 s | **~53 s** (old sweep) → **~1.2–1.3 s** with the W29 fix | MISS → PASS |

Cold-start notes (method: `scripts/coldstart.ps1` kills the app, spawns it, polls
for the hidden popup window and matches the app's own log lines `rebuffer starting`
and `hotkey Alt+V registered` — the last backend step before tray install — against
the spawn time):

- Process spawn ~35–60 ms; hidden popup window exists ~290–310 ms; `rebuffer
  starting` ~1.0 s after spawn (Tauri + WebView2 init before `setup`).
- With a small store the `starting → hotkey registered` gap is 28–48 ms, so
  tray-ready lands at ~1.05 s.
- The 8–50 s stalls were the **startup integrity sweep** (`janitor::startup_sweep`):
  for every blob file on disk it ran a `SELECT COUNT(*) FROM items WHERE hash = ?`
  plus a `SELECT COUNT(*) FROM item_formats WHERE blob_path LIKE '%<hash>'` (a
  leading-wildcard LIKE that cannot use an index and scans the whole table) — at
  10 000 items that was ~30 000 queries. **W29 fix:** the reference set is read
  once into two in-memory `HashSet`s (one `SELECT DISTINCT hash FROM items`, one
  `SELECT blob_path FROM item_formats`), and the blob tree is walked once with a
  parallel work-stealing `read_dir` traversal; part 2 (rows whose blob is
  missing) is a set lookup per row against the set the walk already built, so the
  tree is stat'd once rather than once per row. The old `Store::open
  (Cold Start + Sweep) 24.56 ms` figure from the benchmark below did **not**
  reproduce against a store that actually has one blob file per item (see the
  corrected row); it predates the per-file sweep or ran without the files present.
- Tray-ready was taken as the `hotkey registered` log line; `tray::install` runs
  immediately after it in `setup`.

## Benchmark Results

The 10 000-item numbers below were re-measured for W29 with a store that has a
real blob file (and, for image items, a thumbnail) per row, plus on-disk format
blobs and orphan/temp/missing-blob corruption to sweep (see the `#[ignore]`
benchmark `store::janitor::tests::benchmark_startup_sweep_10k`). Two runs each:

| Operation | Target | Measured Latency | Notes |
|---|---|---|---|
| **Bulk Ingestion (10,000 items)** | — | **863.80 ms** | 10k items inserted across batched transactions |
| **`Store::open` — OLD sweep (10k items, debug)** | < 500 ms | **106 620 ms** | Per-file COUNT + leading-wildcard LIKE; the 53 s app cold start |
| **`Store::open` — W29 sweep (10k items, debug)** | < 500 ms | **308 ms** | Two set reads + one parallel directory walk |
| **`Store::open` — W29 sweep (10k items, release)** | < 500 ms | **224 ms** | 218/224 ms across runs; well inside the 1.5 s cold-start budget after ~1.0 s of Tauri init |
| **`list` (Page of 200 items)** | < 10 ms | **924.10 µs** | Sort: Newest, batch file names resolution in 1 SQL query, no per-row stat syscalls |
| **`search` (FTS5 match over 10k items)** | < 20 ms | **570.10 µs** | FTS5 match `items_fts MATCH "keyword_search_9950"*` |
| **`ext_facets` (Extension grouping)** | < 15 ms | **3.45 ms** | Extension aggregation and count over 10,000 items |
| **`tab_counts` (Tab bar badges)** | < 10 ms | **3.54 ms** | Single-pass SQL aggregation query calculating counts for all tabs (all, images, text, links, files, pinned) |

## Performance Observations & Optimizations

1. **N+1 Elimination in `map_rows_to_items`**:
   - Querying file names in a single batch with `WHERE item_id IN (...)` allows loading a 200-item page in under **1 ms** (< 925 µs).
   - Removed thumbnail filesystem stat calls (`exists()`) during page mapping, relying on startup integrity verification.
2. **FTS5 Search**:
   - Sub-millisecond (0.57 ms) search time across 10,000 items using SQLite FTS5 with prefix matching and rank ordering.
3. **Cold Startup & Integrity Sweep (W29)**:
   - The sweep used to run one or two SQL queries per blob file on disk — a
     `COUNT(*)` per blob/thumb and a leading-wildcard `LIKE '%<hash>'` per blob
     that scans the whole `item_formats` table. At 10,000 items that is ~30,000
     queries, thousands of them full table scans: the 53 s cold start.
   - It now reads the full reference set with **two** queries
     (`SELECT DISTINCT hash FROM items`, `SELECT blob_path FROM item_formats`
     — the file-name component IS the hash, so the comparison is exact, not a
     suffix LIKE), then walks the `blobs/` tree once with a parallel
     work-stealing `read_dir` traversal (entries classified from the directory
     listing's own attributes, no extra syscalls). Part 2 — rows whose primary
     blob is missing — is a `HashSet` lookup per row against the set the walk
     already built, so the tree is stat'd once instead of once per row.
   - Measured `Store::open` at 10,000 items: **106,620 ms before → 308 ms
     (debug) / 224 ms (release)** on this machine; the directory walk is the
     remaining cost and is I/O-bound.
   - The sweep stays synchronous (inside `Store::open`, before the clipboard
     listener starts): `insert_capture` writes a blob to disk *before* the row
     referencing it is inserted, so a background sweep could delete a fresh blob
     whose row is not yet committed. At the corrected cost that race is not
     worth introducing.
