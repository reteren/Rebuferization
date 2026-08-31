// Storage behaviour harness for W38. Drives the real Rebuffer store code
// (`rebuffer_lib::store`) against store roots passed on the command line, so
// a VHD mounted at a path we control can be filled and dismounted without
// touching the live user's store.
//
// Each subcommand prints `key=value` lines; an expected error is printed as
// `..._ERR=<the message>` and the command still exits 0 (the orchestrator
// grades the lines). A harness panic is caught per-operation and printed as
// `PANIC=...` rather than silently killing the run.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use rebuffer_lib::capture::Capture;
use rebuffer_lib::error::AppResult;
use rebuffer_lib::model::{Filter, RetentionPolicy, Sort};
use rebuffer_lib::store::Store;

fn guard<F: FnOnce() -> R, R>(what: &str, f: F) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            println!("PANIC={what} {msg}");
            std::process::exit(3);
        }
    }
}

fn open_store(root: &Path) -> AppResult<Store> {
    Store::open(root)
}

/// Recursive walk; returns every file path under `dir`.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn db_count(store: &Store) -> i64 {
    store
        .conn()
        .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .unwrap_or(-1)
}

fn db_integrity(store: &Store) -> String {
    store
        .conn()
        .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|_| "ERR".into())
}

fn used_bytes(store: &Store) -> i64 {
    store
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(byte_size),0) FROM items WHERE is_reference = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1)
}

/// Full dump of a store's on-disk + in-DB state, used as evidence after
/// failed operations.
fn dump_state(store: &Store, label: &str) {
    let root = store.root();
    let files = walk_files(&root.join("blobs"));
    let temps = files
        .iter()
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let s = n.to_string_lossy();
                    s.contains(".tmp.") || s.ends_with(".tmp")
                })
                .unwrap_or(false)
        })
        .count();
    let blobs = files
        .iter()
        .filter(|p| !p.starts_with(&root.join("blobs").join("thumbs")))
        .count();
    let thumbs = files
        .iter()
        .filter(|p| p.starts_with(&root.join("blobs").join("thumbs")))
        .count();
    println!("STATE[{label}] count={}", db_count(store));
    println!("STATE[{label}] used_bytes={}", used_bytes(store));
    println!("STATE[{label}] integrity={}", db_integrity(store));
    println!("STATE[{label}] blob_files={blobs} thumb_files={thumbs} temp_files={temps}");
    let orph_rows: i64 = store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM items WHERE is_reference = 0 AND blob_path IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    println!("STATE[{label}] rows_with_blob_path={orph_rows}");
}

fn insert_capture(store: &Store, n_bytes: usize) -> AppResult<i64> {
    let cap = Capture::text("x".repeat(n_bytes.max(1)));
    Ok(store.insert_capture(cap)?.id)
}

// ---------------------------------------------------------------------------
// subcommands
// ---------------------------------------------------------------------------

fn store_open(root: &Path) {
    println!("CALL store_open({})", root.display());
    let r = guard("store_open", || open_store(root));
    match r {
        Ok(store) => {
            println!("RESULT store_open=ok root_exists={}", root.exists());
            dump_state(&store, "fresh");
        }
        Err(e) => println!("RESULT store_open_ERR={e}"),
    }
}

fn seed(root: &Path, n: usize) {
    println!("CALL seed({}, {n})", root.display());
    let store = match guard("seed.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT seed_open_ERR={e}");
            return;
        }
    };
    let mut ok = 0;
    for i in 0..n {
        let body = format!("seed item {i}: {}", "y".repeat(200));
        if store.insert_capture(Capture::text(body)).is_ok() {
            ok += 1;
        }
    }
    println!("RESULT seed={ok}/{n}");
    dump_state(&store, "after_seed");
}

/// Seeds items with a large *inline* clipboard format (stored in the DB, not
/// as a blob file). Deleting such an item makes the SQLite WAL grow by the
/// format's size, which is what lets a volume that is "as full as it gets"
/// actually block a janitor delete.
fn seed_formats(root: &Path, n: usize, fmt_bytes: usize) {
    println!("CALL seed_formats({}, {n} items, {fmt_bytes}B inline format)", root.display());
    let store = match guard("seed_formats.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT seed_formats_open_ERR={e}");
            return;
        }
    };
    let mut ok = 0;
    for i in 0..n {
        let mut cap = Capture::text(format!("with format {i}: {}", "y".repeat(100)));
        cap.formats.push(rebuffer_lib::capture::CapturedFormat {
            format: "Rich Text Format".into(),
            bytes: vec![0xAB; fmt_bytes],
        });
        if store.insert_capture(cap).is_ok() {
            ok += 1;
        }
    }
    println!("RESULT seed_formats={ok}/{n}");
    dump_state(&store, "after_seed_formats");
}

fn insert(root: &Path, n_bytes: usize) {
    println!("CALL insert({}, {n_bytes} bytes)", root.display());
    let store = match guard("insert.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT insert_open_ERR={e}");
            return;
        }
    };
    let before = db_count(&store);
    println!("RESULT insert_before_count={before}");
    let r = guard("insert_capture", || insert_capture(&store, n_bytes));
    match r {
        Ok(id) => {
            println!("RESULT insert=ok id={id}");
        }
        Err(e) => println!("RESULT insert_ERR={e}"),
    }
    let after = db_count(&store);
    println!("RESULT insert_after_count={after} (delta {})", after - before);
    dump_state(&store, "post_insert");
}

fn backdate(root: &Path, days: i64) {
    let store = match guard("backdate.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT backdate_open_ERR={e}");
            return;
        }
    };
    let off = days * 24 * 60 * 60 * 1000;
    let n = store
        .conn()
        .execute("UPDATE items SET created_at = created_at - ?1", [off])
        .unwrap_or(0);
    println!("RESULT backdate_rows={n}");
}

fn cleanup(root: &Path, retention_days: u32, max_store_bytes: Option<i64>) {
    println!(
        "CALL cleanup({}, retention={retention_days}, cap={max_store_bytes:?})",
        root.display()
    );
    let store = match guard("cleanup.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT cleanup_open_ERR={e}");
            return;
        }
    };
    store.set_retention_policy(RetentionPolicy {
        retention_days,
        max_store_bytes,
    });
    let before_count = db_count(&store);
    let r = guard("run_cleanup", || store.run_cleanup(None));
    match r {
        Ok(res) => println!(
            "RESULT cleanup=ok removed={} freed={} (count before={before_count})",
            res.removed_items, res.freed_bytes
        ),
        Err(e) => println!(
            "RESULT cleanup_ERR={e} (count before={before_count})"
        ),
    }
    dump_state(&store, "post_cleanup");
}

fn insert_unique(store: &Store, tag: &str, n_bytes: usize) -> AppResult<i64> {
    let mut body = format!("{tag}:{}:", "u".repeat(n_bytes.max(64)));
    body.push_str(&format!("nonce={}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)));
    Ok(store.insert_capture(Capture::text(body))?.id)
}

/// Phase 3: volume disconnected while running, then reconnected.
fn disconnect(root: &Path, marker: &Path) {
    println!("CALL disconnect({}, marker={})", root.display(), marker.display());
    let store = match guard("disc.open", || open_store(root)) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT disc_open_ERR={e}");
            return;
        }
    };
    match guard("disc.insert_pre", || insert_unique(&store, "pre", 1024)) {
        Ok(id) => println!("RESULT pre_dismount_insert=ok id={id}"),
        Err(e) => println!("RESULT pre_dismount_insert_ERR={e}"),
    }
    println!("READY_BEFORE_DISMOUNT");

    wait_marker(marker, "dismounted");
    println!("MARKER=dismounted seen");

    let r = guard("disc.insert_post", || insert_unique(&store, "post", 1024));
    match r {
        Ok(id) => println!("RESULT post_dismount_insert=ok id={id}"),
        Err(e) => println!("RESULT post_dismount_insert_ERR={e}"),
    }
    let r = guard("disc.list_post", || {
        store.list(&Filter::default(), Sort::Newest, 0, 10)
    });
    match r {
        Ok(v) => println!("RESULT post_dismount_list=ok rows={}", v.len()),
        Err(e) => println!("RESULT post_dismount_list_ERR={e}"),
    }
    let r = guard("disc.cleanup_post", || store.run_cleanup(None));
    match r {
        Ok(res) => println!(
            "RESULT post_dismount_cleanup=ok removed={}",
            res.removed_items
        ),
        Err(e) => println!("RESULT post_dismount_cleanup_ERR={e}"),
    }
    let r = guard("disc.reopen_post", || open_store(root));
    match r {
        Ok(_) => println!("RESULT post_dismount_reopen=ok"),
        Err(e) => println!("RESULT post_dismount_reopen_ERR={e}"),
    }

    wait_marker(marker, "remounted");
    println!("MARKER=remounted seen");

    let r = guard("disc.insert_rem", || insert_unique(&store, "rem", 1024));
    match r {
        Ok(id) => println!("RESULT post_remount_insert=ok id={id}"),
        Err(e) => println!("RESULT post_remount_insert_ERR={e}"),
    }
    let r = guard("disc.list_rem", || {
        store.list(&Filter::default(), Sort::Newest, 0, 10)
    });
    match r {
        Ok(v) => println!("RESULT post_remount_list=ok rows={}", v.len()),
        Err(e) => println!("RESULT post_remount_list_ERR={e}"),
    }
    // Release the stale connection, then try a fresh open — does the store
    // recover once nothing holds the dead file handles?
    drop(store);
    let r = guard("disc.reopen_rem", || open_store(root));
    match r {
        Ok(s) => {
            println!("RESULT post_remount_reopen_after_drop=ok");
            match guard("disc.cleanup_rem", || s.run_cleanup(None)) {
                Ok(res) => println!(
                    "RESULT post_remount_cleanup=ok removed={}",
                    res.removed_items
                ),
                Err(e) => println!("RESULT post_remount_cleanup_ERR={e}"),
            }
            match guard("disc.insert_rem_fresh", || insert_unique(&s, "rem2", 2048)) {
                Ok(id) => println!("RESULT post_remount_insert_fresh=ok id={id}"),
                Err(e) => println!("RESULT post_remount_insert_fresh_ERR={e}"),
            }
        }
        Err(e) => println!("RESULT post_remount_reopen_after_drop_ERR={e}"),
    }
    println!("DONE");
}

fn wait_marker(marker: &Path, want: &str) {
    loop {
        if let Ok(s) = std::fs::read_to_string(marker) {
            if s.trim() == want {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Phase 5: relocation (uses the real `janitor::relocate`, which needs a
/// Tauri AppHandle — a mocked app built via `tauri::test` can't provide the
/// Wry-typed handle the signature wants, so we build a real (never-run)
/// Tauri app to get one).
fn relocate(src: &Path, dst: &Path) {
    println!("CALL relocate({}, {})", src.display(), dst.display());
    let app = match tauri::Builder::default()
        .build(tauri::generate_context!())
    {
        Ok(a) => Arc::new(a),
        Err(e) => {
            println!("RESULT relocate_tauri_build_ERR={e}");
            return;
        }
    };
    let store = match open_store(src) {
        Ok(s) => s,
        Err(e) => {
            println!("RESULT relocate_open_src_ERR={e}");
            return;
        }
    };
    let src_existed = src.exists();
    println!("RESULT relocate_src_exists_before={src_existed}");
    let r = guard("relocate", || {
        rebuffer_lib::store::janitor::relocate(&app.handle(), &store, dst)
    });
    match r {
        Ok(()) => println!("RESULT relocate=ok"),
        Err(e) => println!("RESULT relocate_ERR={e}"),
    }
    // Post-state: source gone, target populated, store still usable.
    println!("RESULT relocate_src_exists_after={}", src.exists());
    println!("RESULT relocate_dst_exists={}", dst.exists());
    if dst.exists() {
        println!("RESULT relocate_dst_has_db={}", dst.join("rebuffer.db").exists());
        if let Ok(dstore) = Store::open(dst) {
            dump_state(&dstore, "post_relocate_target");
        } else {
            println!("RESULT relocate_dst_reopen_ERR");
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: storage-harness <cmd> <path> [args...]");
        return ExitCode::FAILURE;
    }
    let cmd = &args[1];
    let p1 = PathBuf::from(&args[2]);
    match cmd.as_str() {
        "store-open" => store_open(&p1),
        "seed" => seed(&p1, args[3].parse().unwrap_or(10)),
        "seed-formats" => seed_formats(
            &p1,
            args[3].parse().unwrap_or(5),
            args[4].parse().unwrap_or(60000),
        ),
        "insert" => insert(&p1, args[3].parse().unwrap_or(1024)),
        "backdate" => backdate(&p1, args[3].parse().unwrap_or(1)),
        "cleanup" => {
            let days = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
            let cap = args.get(4).and_then(|s| s.parse::<i64>().ok());
            cleanup(&p1, days, cap);
        }
        "disconnect" => disconnect(&p1, Path::new(args.get(3).map(String::as_str).unwrap_or("marker.txt"))),
        "relocate" => relocate(&p1, Path::new(&args[3])),
        other => {
            eprintln!("unknown cmd: {other}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}