# Rebuffer Store Performance Report (10,000 Items)

This document records the baseline persistence performance metrics for the Rebuffer SQLite store and content-addressed blob subsystem, fulfilling ROADMAP Phase 7 requirements.

## Test Environment

- **CPU**: AMD Ryzen 7 7800X3D (8 Cores, 16 Logical Processors)
- **Operating System**: Microsoft Windows 11 Enterprise (64-bit)
- **Rust Profile**: `dev` test build (`[unoptimized + debuginfo]`)
- **Storage Subsystem**: NVMe SSD / NTFS (temp directory via `tempfile`)
- **Dataset Size**: 10,000 items (mix of plain text, rich text, images, multi-file collections, and references)

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
