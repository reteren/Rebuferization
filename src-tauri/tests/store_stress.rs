use std::time::Instant;
use tempfile::tempdir;

use rebuffer_lib::model::{Filter, Sort};
use rebuffer_lib::store::Store;

#[test]
#[ignore]
fn test_store_stress_10000_items() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    println!("\n=== REBUFFER STORE STRESS TEST (10,000 ITEMS) ===");
    println!("Store root: {:?}", root);

    let insert_start = Instant::now();
    {
        let store = Store::open(root).unwrap();
        let conn = store.conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        conn.execute_batch("BEGIN TRANSACTION;").unwrap();
        for i in 1..=10000 {
            let kind = match i % 4 {
                0 => "text",
                1 => "image",
                2 => "file",
                _ => "text",
            };
            let sub_kind = if i % 5 == 0 {
                Some("link")
            } else {
                Some("plain")
            };
            let ext = match i % 4 {
                0 => Some("TXT"),
                1 => Some("PNG"),
                2 => Some("PDF"),
                _ => Some("RS"),
            };
            let hash = format!("{:064x}", i);
            let preview = format!("Item preview number {} with keyword_search_{}", i, i);
            let created = now - (i * 1000);
            let pinned = if i % 50 == 0 { 1 } else { 0 };

            conn.execute(
                "INSERT INTO items (
                    kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path,
                    title, preview_text, ext, mime, byte_size, width, height, duration_ms,
                    source_app, copy_count, created_at, first_seen_at, last_used_at, pinned
                ) VALUES (?1, ?2, ?3, NULL, NULL, 0, NULL, NULL, ?4, ?5, NULL, 1024, NULL, NULL, NULL, 'test.exe', 1, ?6, ?6, NULL, ?7)",
                rusqlite::params![kind, sub_kind, hash, preview, ext, created, pinned],
            ).unwrap();

            if i % 1000 == 0 {
                conn.execute_batch("COMMIT; BEGIN TRANSACTION;").unwrap();
            }
        }
        conn.execute_batch("COMMIT;").unwrap();
    }
    let insert_duration = insert_start.elapsed();
    println!("Inserted 10,000 items in {:?}", insert_duration);

    // 1. Measure Store::open (cold startup + startup integrity sweep on 10,000 items)
    let open_start = Instant::now();
    let store = Store::open(root).unwrap();
    let open_duration = open_start.elapsed();

    // 2. Measure list page of 200 items
    let list_start = Instant::now();
    let page = store
        .list(&Filter::default(), Sort::Newest, 0, 200)
        .unwrap();
    let list_duration = list_start.elapsed();
    assert_eq!(page.len(), 200);

    // 3. Measure FTS search across 10,000 items
    let search_start = Instant::now();
    let search_results = store
        .search("keyword_search_9950", &Filter::default(), 200)
        .unwrap();
    let search_duration = search_start.elapsed();
    assert!(!search_results.is_empty());

    // 4. Measure extension facets call
    let facets_start = Instant::now();
    let facets = store.ext_facets(&Filter::default()).unwrap();
    let facets_duration = facets_start.elapsed();
    assert!(!facets.is_empty());

    // 5. Measure tab_counts call
    let tab_counts_start = Instant::now();
    let tab_counts = store.tab_counts().unwrap();
    let tab_counts_duration = tab_counts_start.elapsed();
    assert_eq!(tab_counts.all, 10000);

    println!("\n| Operation | Target | Measured | Notes |");
    println!("|---|---|---|---|");
    println!("| `Store::open` (sweep on 10k items) | < 500 ms | {:.2?} | Cold startup + integrity sweep |", open_duration);
    println!(
        "| `list` (page of 200 items) | < 10 ms | {:.2?} | Sort: Newest, batch file names |",
        list_duration
    );
    println!(
        "| `search` (FTS5 over 10k items) | < 20 ms | {:.2?} | Query: 'keyword_search_9950' |",
        search_duration
    );
    println!(
        "| `ext_facets` | < 15 ms | {:.2?} | Aggregation over 10k extensions |",
        facets_duration
    );
    println!(
        "| `tab_counts` | < 10 ms | {:.2?} | Single-pass aggregate across 10k items |",
        tab_counts_duration
    );
    println!("\n=================================================");
}
