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
| Idle RAM, main process | < 60 MB | **45.8 MB median, 45.9 MB max** (15 samples @ 4 s) | PASS |
| Idle RAM, incl. WebView2 children | — | ~476 MB total (children ≈ 430 MB) | (context) |
| Idle CPU | 0% | **median 0%, max 0.36% of one core** | PASS (one 0.36 % sample) |
| Cold start to tray-ready | < 1.5 s | **~1.05 s** with a ~34-item store | PASS |
| Cold start to tray-ready, 7 043-item store | < 1.5 s | **~9.5 s** | MISS |
| Cold start to tray-ready, 10 000-item store | < 1.5 s | **~53 s** | MISS |

Cold-start notes (method: `scripts/coldstart.ps1` kills the app, spawns it, polls
for the hidden popup window and matches the app's own log lines `rebuffer starting`
and `hotkey Alt+V registered` — the last backend step before tray install — against
the spawn time):

- Process spawn ~35–60 ms; hidden popup window exists ~290–310 ms; `rebuffer
  starting` ~1.0 s after spawn (Tauri + WebView2 init before `setup`).
- With a small store the `starting → hotkey registered` gap is 28–48 ms, so
  tray-ready lands at ~1.05 s.
- The 8–50 s stalls are the **startup integrity sweep** (`janitor::startup_sweep`):
  for every blob file on disk it runs a `SELECT COUNT(*) FROM items WHERE hash = ?`
  plus a `Path::exists()` per row. Measured standalone on the 7 043-file store the
  COUNT loop alone took **8.5 s** (one sqlite3 session, 7 043 statements); at
  10 000 items the app took 53 s from `starting` to `hotkey registered`. The
  existing `Store::open (Cold Start + Sweep) 24.56 ms` figure from the benchmark
  below does **not** reproduce against a store that actually has one blob file per
  item — it likely predates the per-file sweep or ran without the files present.
- Tray-ready was taken as the `hotkey registered` log line; `tray::install` runs
  immediately after it in `setup`.

## Benchmark Results

| Operation | Target | Measured Latency | Notes |
|---|---|---|---|
| **Bulk Ingestion (10,000 items)** | — | **863.80 ms** | 10k items inserted across batched transactions |
| **`Store::open` (Cold Start + Sweep)** | < 500 ms | **24.56 ms** | Cold connection open, PRAGMA checks, migration check, and startup integrity sweep across 10k items |
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
3. **Cold Startup & Integrity Sweep**:
   - Completed in ~24 ms, sweeping orphan temp files, missing blobs, and unreferenced assets without blocking UI initialization.
