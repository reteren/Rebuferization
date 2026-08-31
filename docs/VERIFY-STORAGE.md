# Rebuffer Storage Verification — Volume Full / Disappearing (W38)

ROADMAP Phase 7: "behaviour when the store volume is full or disconnected".
First time this has been tested. Everything below was measured live against
the **real store code** (`rebuffer_lib`) on a **VHD I created, mounted, filled,
dismounted and deleted**, so the user's machine, `%APPDATA%\Rebuffer`, and
their `settings.json` were never touched.

## How a separate "instance" was pointed at a controlled store

The app cannot be run a second time against a test store: the settings path is
hardcoded to `%APPDATA%\Rebuffer\settings.json` (`settings.rs` / `lib.rs`
setup, no environment override exists), and `tauri-plugin-single-instance`
forwards any second launch to the already-running instance. Pointing the real
app at a test store would require editing the live `settings.json` (forbidden
here — the user is using the app) or stopping their instance.

Instead, a standalone harness under `scripts/storage/` depends on the real
crate by path (`rebuffer = { path = "../../src-tauri" }`) and drives the exact
same persistence code — `Store::open`, `insert_capture`, `run_cleanup`,
`janitor::relocate`, `Store::conn` — against store roots I create on the VHD.
`relocate` needs a `tauri::AppHandle`, which the harness obtains by building a
never-run Tauri app (`build.rs` + a minimal `tauri.conf.json` +
`tauri::generate_context!`); `tauri::test`'s mock can't provide the Wry-typed
handle the signature wants. So: same store code, my paths.

Volumes: `diskpart` creates/attaches/detaches a 128 MB fixed VHD at
`scripts/storage\rbf-test-W.vhd`, mounted as **W:**, NTFS. `New-VHD`/`Mount-VHD`
were avoided (they need Hyper-V). `scripts\storage\run.ps1` orchestrates all
phases; raw output is in `scripts\storage\out\log.txt`.

## Q1 — Full volume on capture

Volume filled to **139,264 bytes free** (0.1 % of 128 MB; see the note on the
NTFS floor below). The store already held 5 items.

| Operation | Result | Post-state |
|---|---|---|
| `insert_capture` 1 MB text blob | **`Err(io: os error 112)`** — ENOSPC, surfaced as an error, **no panic, no crash** | count 5→5 (delta **0**), `rows_with_blob_path` 5→5, `integrity_check` = ok |
| 4 KB text blob | `Ok` (id 6) | count +1, temp file from the failed write gone (swept by the next `Store::open`) |
| 256 B text blob | `Ok` (id 7) | count +1 |

**The design promise holds:** the blob is written to a temp file and renamed
*before* the row insert, so a failed write leaves **no row** pointing at a
missing/partial blob — the database stays consistent (count and
`rows_with_blob_path` unchanged, integrity ok). **One caveat:** the failed
1 MB write left **a partial `.tmp.` blob file on the volume**
(`blob_files` 5→6, `temp_files=1`). `write_blob` cleans up the temp only on
the *rename* error path, not on the mid-write ENOSPC path; the leftover is
removed by the next startup integrity sweep (observed: `temp_files` 1→0 on
the very next `Store::open`), but it lingers for the session.

App-level (from code, `clipboard/listener.rs:247`): the capture is logged as
`tracing::error!("Failed to insert capture into store: …")` and **silently
dropped — the user is not told**. The app stays alive and keeps capturing
(small captures continued to succeed).

## Q2 — Full volume during the janitor

Two filesystem realities shape this test:

- **You cannot make a "few KB free" volume with a normal file.** NTFS and
  FAT32 both keep ~128 KB of free headroom for metadata that regular
  allocation cannot consume (NTFS reserves it; FAT32's FAT/root can't shrink
  below it). "Full" here means ~128–139 KB free — which is the realistic
  "disk full" a user ever meets.
- The janitor's deletes (age sweep and size-cap prune) only need a few KB of
  SQLite WAL headroom and *free space as they go*, so at the floor they
  succeed.

| Case | Free at cleanup | Result |
|---|---|---|
| Age sweep, 17 plain items backdated 31 d | 131,072 B | `removed=17`, blobs 17→0, integrity ok — **succeeded** |
| Age sweep, 5 items with 60 KB inline formats, backdated | 131,072 B | `removed=5`, blobs 5→0, integrity ok — **succeeded** |
| Size-cap prune, cap 300 B < 575 B used, 60 KB-inline items | 131,072 B | `removed=3 freed=345`, count 5→2, blobs 5→2, integrity ok — **succeeded** |
| Same prune after the volume is freed (recovery) | ~118 MB | `removed=0` (nothing left over cap), clean |

`delete_items` is a **single SQLite transaction** (commit-then-unlink in
`queries.rs`), so even a failed delete would roll back whole — no half-cleaned
store. No failed-delete path could be provoked at the realistic floor (the
60 KB-inline construction did not exceed the WAL headroom because SQLite frees
BLOB pages via the free-list, not by appending their content to the WAL); that
specific failure mode is untestable without a genuinely <4 KB-free volume,
which Windows filesystems do not allow a regular file to produce.

## Q3 — Volume disconnected while running

The harness opened the store on W: (22 items), inserted one item, then the VHD
was **taken offline while the store connection was open** (the volume vanished
mid-session) and later reattached.

| Moment | Operation | Result |
|---|---|---|
| before dismount | insert 1 KB | `Ok` (id 26) |
| **volume gone** | insert 1 KB | `Err(io: os error 3)` — path not found, clean error, no panic |
| volume gone | `list` | **`Ok` with 0 rows** — reads silently return empty, not an error |
| volume gone | `run_cleanup` | `Ok removed=0` (no crash) |
| volume gone | fresh `Store::open` | `Err(io: os error 3)` |
| **reattached** | insert (stale connection) | `Err(database: disk I/O error)` — the stale connection is wedged |
| reattached | `list` (stale connection) | `Ok` 0 rows (still empty) |
| reattached | fresh `Store::open` *while the stale connection is still alive* | `Err(database: disk I/O error)` — the dead handle blocks the wipe-and-retry in `open_database` |
| reattached | **drop the stale connection, then** fresh `Store::open` | **`Ok`**, insert works (id 27) |

Findings:

- **Never crashes, never hangs, never panics** — every store call returns a
  clean error (or, for reads, an empty result).
- **The app does not recover on its own.** The store connection lives for the
  process lifetime and is never reopened, so after a disconnect+reconnect the
  running app is wedged: new captures error (`disk I/O error`), reads return
  empty, the hourly janitor fails silently (its errors are ignored in
  `store/mod.rs`). Only a **process restart** recovers — and a restart does
  recover (the fresh-open-after-drop case proves the volume's data is intact).
- The frontend, inferred from code (IPC errors → `items` store catch →
  `loading=false`, list unchanged): a popup opened while disconnected shows
  the **empty state** (or stale items if it was already open); it does not
  freeze or crash. This UI path was not live-tested (see *Could not test*).

## Q4 — Volume missing at startup

| Store path | `Store::open` result |
|---|---|
| `W:\store` (W: dismounted) | `Err(io: os error 3)` — path not found |
| `W:\` (drive letter gone) | `Err(io: os error 3)` |
| `Z:\no-such-drive\store` | `Err(io: os error 3)` |
| an existing empty directory | `Ok` — fresh store created |

App-level (from code, `lib.rs:95`): setup does `Store::open(&store_root)?`
with **no fallback**. A store path on a missing volume makes the app fail to
start. The tray app never appears, so the user cannot open Settings to fix the
path — **the exact unrecoverable state ROADMAP §7 warned about.** Recovery
requires hand-editing `settings.json`. This is a genuine gap.

## Q5 — Relocation to a too-small target

Source store: 40 items (~8.5 KB blobs + ~150 KB DB). Target: W: with **139,264
bytes free**.

| Target state | Result |
|---|---|
| `W:\reloc-target` (directory **does not exist**) | `relocate_ERR=database: disk I/O error` — **the SPEC 3.5 refusal did NOT fire**; relocation started copying and failed partway, leaving a partial target (a `rebuffer.db` present but **0 items** where the source had 40); the source stayed intact (40 items, verified after) |
| `W:\reloc-target-preexisting` (directory **pre-created**) | `relocate_ERR=Target volume has insufficient free space (available: 0 bytes, required: 142970 bytes)` — **the SPEC refusal fires correctly**; source intact |
| `W:\reloc-target2` (roomy target) | `relocate=ok` — source removed, target holds all 30 items, integrity ok |
| `Z:\no-such-drive\target` (drive missing) | `Err(io: os error 3)`, source intact |

Root cause (precise): the guard in `janitor::relocate` calls
`get_free_disk_space(target)` → `GetDiskFreeSpaceExW(target)` with the **raw
target path**, which does not exist at check time (the directories are created
*after* the check). The API call fails, the function returns `None`, and the
free-space check is **silently skipped**. The check only works if the target
directory already exists. So the documented "refuse if the target has less
free space than store size × 1.2" (SPEC 3.5) **does not protect the common
relocation case** (a fresh target folder on a nearly-full volume): the copy
proceeds, fails mid-way with a confusing `disk I/O error`, and leaves a
partial, unusable target tree. The source store itself is safe (never touched
until verification passes).

## Findings worth fixing (precise enough to act on)

1. **Q4 — app refuses to start when the store volume is missing** (`lib.rs:95`
   `Store::open(&store_root)?`, no fallback). This is the unrecoverable state.
   Suggested directions for the owner: fall back to the default root
   (`%APPDATA%\Rebuffer`) with a prominent warning banner, or open Settings in
   a degraded "store missing" mode; at minimum, don't let the tray app fail to
   launch.
2. **Q5 — relocation free-space guard is dead for nonexistent target paths**
   (`janitor.rs` `get_free_disk_space(target)` before `create_dir_all`).
   Fix: resolve the target's volume root (walk up to an existing ancestor and
   call `GetDiskFreeSpaceExW` on it), or `create_dir_all(target)` before the
   check, or remove the partial target tree on any mid-copy failure.
3. **Q3 — the app stays wedged after a disconnect+reconnect** until restart:
   the process-long store connection is never reopened, so reads return empty
   and writes error after the volume returns. Consider detecting
   `SQLITE_IOERR` / `os error 3` on a store op and re-running
   `Store::open`/`switch_root`, and/or surfacing a "store unreachable" banner.
4. **Q1 — a failed blob write leaves a partial `.tmp` file** for the session
   (`blobs.rs::write_blob` cleans the temp only on the rename-error path).
   Remove the temp in the write-error path too.
5. Notes: while disconnected, `list` returns `Ok([])` rather than an error
   (the popup would silently show the empty state); the janitor's errors are
   silently ignored (`let _ =` in `store/mod.rs`).

## Could not be tested (honestly)

- **The live popup/tray UI** (banner vs. empty grid vs. frozen window):
  running the real app against a test store is impossible without editing the
  user's live `settings.json` or killing their instance, both forbidden. The
  store-layer behavior above is the real code end-to-end; the UI behavior is
  inferred from the IPC/error flow and marked as such.
- **A genuinely <4 KB-free volume**: NTFS and FAT32 both reserve ~128 KB that
  normal file allocation cannot consume, so the janitor's "SQLITE_FULL
  mid-delete" failure mode could not be provoked (its deletes fit in the
  floor). The same floor means "few KB free" in the brief was approximated by
  ~131–139 KB free.
- **A real user-facing disconnect** of the actual store drive (we used a VHD,
  deliberately).

## Cleanup

The VHD was created, mounted, filled, dismounted mid-test, reattached, and
**deleted at the end** (no `*.vhd` files remain, W: is no longer assigned, no
stray disks or processes). Temp stores under `%TEMP%\opencode\w38` were
removed. `rebuffer.exe` was never launched and `%APPDATA%\Rebuffer` was never
read for testing (verified: the user's store kept its own item count
throughout). Repro tooling remains under `scripts/storage/` (`run.ps1`,
`vhd.ps1`, `src/main.rs`); its build output `target/` is removed after use.