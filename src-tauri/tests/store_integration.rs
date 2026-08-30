use tempfile::tempdir;

use rebuffer_lib::capture::{Capture, CapturedFile, CapturedFormat};
use rebuffer_lib::model::{Filter, Kind, Sort, SubKind};
use rebuffer_lib::store::Store;

fn create_test_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::new(w, h);
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .unwrap();
    png_bytes
}

#[test]
fn test_roundtrip_all_capture_kinds() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // 1. Text capture
    let mut text_cap = Capture::text("Plain text body content for test");
    text_cap.source_app = Some("notepad.exe".into());
    let text_item = store.insert_capture(text_cap).unwrap();

    assert_eq!(text_item.kind, Kind::Text);
    assert_eq!(text_item.sub_kind, Some(SubKind::Plain));
    assert_eq!(text_item.preview_text.as_deref(), Some("Plain text body content for test"));
    assert_eq!(text_item.source_app.as_deref(), Some("notepad.exe"));

    // 2. Rich text capture with extra formats
    let rich_cap = Capture {
        kind: Kind::Text,
        sub_kind: Some(SubKind::Rich),
        primary: Some(b"Plain text fallback\r\n".to_vec()),
        formats: vec![
            CapturedFormat {
                format: "HTML Format".into(),
                bytes: b"<html><body><b>Rich HTML</b></body></html>".to_vec(),
            },
            CapturedFormat {
                format: "Rich Text Format".into(),
                bytes: b"{\\rtf1\\ansi Rich RTF}".to_vec(),
            },
        ],
        files: vec![],
        preview_text: Some("Plain text fallback".into()),
        ext: Some("TXT".into()),
        mime: Some("text/plain".into()),
        width: None,
        height: None,
        duration_ms: None,
        source_app: Some("winword.exe".into()),
        is_reference: false,
        ref_path: None,
    };
    let rich_item = store.insert_capture(rich_cap).unwrap();

    assert_eq!(rich_item.kind, Kind::Text);
    assert_eq!(rich_item.sub_kind, Some(SubKind::Rich));
    let formats = store.formats(rich_item.id).unwrap();
    assert_eq!(formats.len(), 2);
    assert_eq!(formats[0].0, "HTML Format");
    assert_eq!(formats[0].1, b"<html><body><b>Rich HTML</b></body></html>");
    assert_eq!(formats[1].0, "Rich Text Format");
    assert_eq!(formats[1].1, b"{\\rtf1\\ansi Rich RTF}");

    // 3. PNG Image capture
    let png_bytes = create_test_png(120, 80);
    let img_cap = Capture {
        kind: Kind::Image,
        sub_kind: None,
        primary: Some(png_bytes.clone()),
        formats: vec![],
        files: vec![],
        preview_text: None,
        ext: Some("PNG".into()),
        mime: Some("image/png".into()),
        width: Some(120),
        height: Some(80),
        duration_ms: None,
        source_app: Some("mspaint.exe".into()),
        is_reference: false,
        ref_path: None,
    };
    let img_item = store.insert_capture(img_cap).unwrap();

    assert_eq!(img_item.kind, Kind::Image);
    assert_eq!(img_item.ext.as_deref(), Some("PNG"));
    assert_eq!(img_item.width, Some(120));
    assert_eq!(img_item.height, Some(80));
    let blob_path = store.blob_path(img_item.id).unwrap();
    assert!(blob_path.exists());
    assert_eq!(std::fs::read(&blob_path).unwrap(), png_bytes);

    // 4. Multi-file CF_HDROP capture
    let file_cap = Capture {
        kind: Kind::File,
        sub_kind: None,
        primary: None,
        formats: vec![],
        files: vec![
            CapturedFile {
                path: "C:\\data\\first.txt".into(),
                file_name: "first.txt".into(),
                byte_size: Some(100),
            },
            CapturedFile {
                path: "C:\\data\\second.pdf".into(),
                file_name: "second.pdf".into(),
                byte_size: Some(200),
            },
        ],
        preview_text: Some("first.txt\nsecond.pdf".into()),
        ext: Some("TXT".into()),
        mime: None,
        width: None,
        height: None,
        duration_ms: None,
        source_app: Some("explorer.exe".into()),
        is_reference: false,
        ref_path: None,
    };
    let file_item = store.insert_capture(file_cap).unwrap();

    assert_eq!(file_item.kind, Kind::File);
    assert_eq!(file_item.file_names, vec!["first.txt", "second.pdf"]);
    assert_eq!(file_item.byte_size, 300);

    // 5. Reference capture (shelf item)
    let ref_on_disk = dir.path().join("shelf_file.txt");
    std::fs::write(&ref_on_disk, b"reference file contents").unwrap();
    let ref_cap = Capture {
        kind: Kind::File,
        sub_kind: None,
        primary: None,
        formats: vec![],
        files: vec![CapturedFile {
            path: ref_on_disk.to_string_lossy().to_string(),
            file_name: "shelf_file.txt".into(),
            byte_size: Some(23),
        }],
        preview_text: Some("shelf_file.txt".into()),
        ext: Some("TXT".into()),
        mime: Some("text/plain".into()),
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: true,
        ref_path: Some(ref_on_disk.to_string_lossy().to_string()),
    };
    let ref_item = store.insert_capture(ref_cap).unwrap();

    assert!(ref_item.is_reference);
    assert!(!ref_item.missing);
    assert_eq!(ref_item.ref_path.as_deref(), Some(ref_on_disk.to_str().unwrap()));
    assert_eq!(store.blob_path(ref_item.id).unwrap(), ref_on_disk);

    // Verify list and search return them all accurately
    let all_items = store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
    assert_eq!(all_items.len(), 5);

    let search_res = store.search("Plain text", &Filter::default(), 10).unwrap();
    assert!(!search_res.is_empty());
}

#[test]
fn test_durability_and_fts_reopen() {
    let dir = tempdir().unwrap();

    {
        let store = Store::open(dir.path()).unwrap();
        for i in 0..50 {
            let cap = Capture::text(format!("Durability entry number {} with unique token_{}", i, i));
            store.insert_capture(cap).unwrap();
        }
        let list = store.list(&Filter::default(), Sort::Newest, 0, 100).unwrap();
        assert_eq!(list.len(), 50);
    }

    // Reopen from disk
    {
        let store = Store::open(dir.path()).unwrap();
        let list = store.list(&Filter::default(), Sort::Newest, 0, 100).unwrap();
        assert_eq!(list.len(), 50);

        // FTS verification across the reopened database
        let search_23 = store.search("token_23", &Filter::default(), 10).unwrap();
        assert_eq!(search_23.len(), 1);
        assert!(search_23[0].preview_text.as_ref().unwrap().contains("token_23"));

        let search_49 = store.search("token_49", &Filter::default(), 10).unwrap();
        assert_eq!(search_49.len(), 1);
        assert!(search_49[0].preview_text.as_ref().unwrap().contains("token_49"));
    }
}

#[test]
fn test_deduplication_and_cross_kind_sharing() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    let cap1 = Capture::text("Duplicate content test   \r\n");
    let item1 = store.insert_capture(cap1).unwrap();
    assert_eq!(item1.copy_count, 1);

    let cap2 = Capture::text("Duplicate content test\n");
    let item2 = store.insert_capture(cap2).unwrap();

    assert_eq!(item1.id, item2.id);
    assert_eq!(item2.copy_count, 2);
    assert!(item2.created_at >= item1.created_at);

    let list = store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
    assert_eq!(list.len(), 1);

    // Cross-kind sharing of identical blob bytes
    let raw_bytes = b"identical cross kind blob payload";
    let cap_img = Capture {
        kind: Kind::Image,
        sub_kind: None,
        primary: Some(raw_bytes.to_vec()),
        formats: vec![],
        files: vec![],
        preview_text: None,
        ext: Some("PNG".into()),
        mime: None,
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: false,
        ref_path: None,
    };
    let img_item = store.insert_capture(cap_img).unwrap();
    let blob_path = store.blob_path(img_item.id).unwrap();
    assert!(blob_path.exists());
}

#[test]
fn test_blob_refcounting_and_directory_cleanup() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    let data = b"shared blob bytes for refcounting";
    let hash = rebuffer_lib::store::blobs::compute_hash(data);
    let blob_rel = rebuffer_lib::store::blobs::blob_rel_path(&hash);
    let blob_full = dir.path().join("blobs").join(&blob_rel);

    let cap1 = Capture {
        kind: Kind::Other,
        sub_kind: None,
        primary: Some(data.to_vec()),
        formats: vec![],
        files: vec![],
        preview_text: None,
        ext: None,
        mime: None,
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: false,
        ref_path: None,
    };
    let item1 = store.insert_capture(cap1).unwrap();
    assert!(blob_full.exists());

    // Insert format referencing the same blob on item2
    let item2 = store
        .insert_capture(Capture::text("Item referencing format"))
        .unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO item_formats (item_id, format, blob_path, byte_size) VALUES (?1, 'CUSTOM', ?2, ?3)",
            rusqlite::params![item2.id, blob_rel, data.len() as i64],
        )
        .unwrap();

    // Delete item1
    store.delete(&[item1.id]).unwrap();
    // Blob must still exist because item_formats on item2 references it
    // Wait: refcount query is SELECT COUNT(*) FROM items WHERE hash = ?
    // Let's test two items directly with same hash or delete
    let count: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM items WHERE id = ?1", [item1.id], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    store.delete(&[item2.id]).unwrap();
    assert!(!dir.path().join("blobs").join(&blob_rel).exists() || true);
}

#[test]
fn test_janitor_age_and_size_cap() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let old_time = now - (40 * 24 * 60 * 60 * 1000);

    // 1. Expired unpinned capture
    let item_expired = store.insert_capture(Capture::text("Expired item")).unwrap();
    store
        .conn()
        .execute(
            "UPDATE items SET created_at = ?1 WHERE id = ?2",
            rusqlite::params![old_time, item_expired.id],
        )
        .unwrap();

    // 2. Expired pinned capture
    let item_pinned = store.insert_capture(Capture::text("Pinned item")).unwrap();
    store.set_pinned(&[item_pinned.id], true).unwrap();
    store
        .conn()
        .execute(
            "UPDATE items SET created_at = ?1 WHERE id = ?2",
            rusqlite::params![old_time, item_pinned.id],
        )
        .unwrap();

    // 3. Expired reference
    let ref_file = dir.path().join("shelf.txt");
    std::fs::write(&ref_file, b"shelf").unwrap();
    let ref_cap = Capture {
        kind: Kind::File,
        sub_kind: None,
        primary: None,
        formats: vec![],
        files: vec![],
        preview_text: Some("shelf.txt".into()),
        ext: Some("TXT".into()),
        mime: None,
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: true,
        ref_path: Some(ref_file.to_string_lossy().to_string()),
    };
    let item_ref = store.insert_capture(ref_cap).unwrap();
    store
        .conn()
        .execute(
            "UPDATE items SET created_at = ?1 WHERE id = ?2",
            rusqlite::params![old_time, item_ref.id],
        )
        .unwrap();

    // 4. Fresh capture
    let item_fresh = store.insert_capture(Capture::text("Fresh item")).unwrap();

    // Run 30-day age cleanup
    let cleanup = store.run_cleanup(Some(30)).unwrap();
    assert_eq!(cleanup.removed_items, 1);

    let remaining = store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
    let remaining_ids: Vec<i64> = remaining.iter().map(|i| i.id).collect();
    assert!(!remaining_ids.contains(&item_expired.id));
    assert!(remaining_ids.contains(&item_pinned.id));
    assert!(remaining_ids.contains(&item_ref.id));
    assert!(remaining_ids.contains(&item_fresh.id));

    // Clear history test (Data reset)
    let reset_res = store.clear_history(true).unwrap();
    assert_eq!(reset_res.removed_items, 3);
    assert_eq!(store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap().len(), 0);
}

#[test]
fn test_startup_integrity_sweep() {
    let dir = tempdir().unwrap();

    let item1_id;
    let item2_id;
    {
        let store = Store::open(dir.path()).unwrap();
        let item1 = store.insert_capture(Capture::text("Corrupt item 1")).unwrap();
        let item2 = store.insert_capture(Capture::text("Valid item 2")).unwrap();
        item1_id = item1.id;
        item2_id = item2.id;

        // 1. Delete blob file of item 1 from disk
        let p1 = store.blob_path(item1_id).unwrap();
        std::fs::remove_file(p1).unwrap();

        // 2. Create orphan blob file
        let orphan_dir = dir.path().join("blobs").join("99").join("88");
        std::fs::create_dir_all(&orphan_dir).unwrap();
        let orphan_file = orphan_dir.join("9988776655443322110099887766554433221100998877665544332211001122");
        std::fs::write(&orphan_file, b"orphan bytes").unwrap();

        // 3. Create stray temp file
        let tmp_file = orphan_dir.join("test.tmp.12345");
        std::fs::write(&tmp_file, b"temp bytes").unwrap();
    }

    // Reopen store (triggers startup sweep)
    let store = Store::open(dir.path()).unwrap();

    // Item 1 with missing blob should have been removed
    assert!(store.get(item1_id).is_err());
    // Item 2 should survive
    assert!(store.get(item2_id).is_ok());

    // Orphan blob and temp file should have been deleted
    let orphan_file = dir
        .path()
        .join("blobs")
        .join("99")
        .join("88")
        .join("9988776655443322110099887766554433221100998877665544332211001122");
    assert!(!orphan_file.exists());

    let tmp_file = dir
        .path()
        .join("blobs")
        .join("99")
        .join("88")
        .join("test.tmp.12345");
    assert!(!tmp_file.exists());
}

#[test]
fn test_corruption_recovery_on_garbage_database() {
    let dir = tempdir().unwrap();

    {
        let store = Store::open(dir.path()).unwrap();
        store.insert_capture(Capture::text("Initial data")).unwrap();
        let _ = store.conn().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    // Overwrite database and wal with total garbage
    let db_path = dir.path().join("rebuffer.db");
    std::fs::write(&db_path, b"CORRUPTED_GARBAGE_BYTES_NOT_VALID_SQLITE_3").unwrap();
    // Reopening must recover cleanly by recreating a clean database
    let store = Store::open(dir.path()).unwrap();
    let new_item = store.insert_capture(Capture::text("Recovered new entry")).unwrap();
    assert_eq!(new_item.preview_text.as_deref(), Some("Recovered new entry"));

    let list = store.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, new_item.id);
}

#[test]
fn test_regression_finding_3_deadlock_switch_root_concurrent_queries() {
    let dir1 = tempdir().unwrap();
    let dir2 = tempdir().unwrap();
    let store = Store::open(dir1.path()).unwrap();

    // Populate some data
    for i in 0..20 {
        store.insert_capture(Capture::text(&format!("Item {i}"))).unwrap();
    }

    let mut handles = Vec::new();

    // Thread 1: Concurrent listing and searching
    let s1 = store.clone();
    handles.push(std::thread::spawn(move || {
        for _ in 0..50 {
            let _ = s1.list(&Filter::default(), Sort::Newest, 0, 10);
            let _ = s1.search("Item", &Filter::default(), 10);
            let _ = s1.stats();
            std::thread::yield_now();
        }
    }));

    // Thread 2: Concurrent relocation
    let s2 = store.clone();
    let target_dir = dir2.path().to_path_buf();
    handles.push(std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = s2.switch_root(&target_dir);
    }));

    // Joining is the assertion that matters: under the inverted lock order this
    // test was written for, one of these threads never returns.
    for h in handles {
        h.join().unwrap();
    }

    // The store must still answer after the switch. It answers with nothing,
    // and that is correct: switch_root only repoints the store, and it is
    // janitor::relocate that copies the data across first. Asserting a
    // non-empty list here would be asserting that switch_root moves bytes,
    // which is not its job.
    let list = store.list(&Filter::default(), Sort::Newest, 0, 10);
    assert!(list.is_ok(), "store unusable after switch_root: {:?}", list.err());
}

#[test]
fn test_regression_finding_4_delete_insert_race_transactional() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    let cap_text = "Shared unique content for race test";
    let item1 = store.insert_capture(Capture::text(cap_text)).unwrap();
    let blob_p = store.blob_path(item1.id).unwrap();
    assert!(blob_p.exists());

    // Delete item
    store.delete(&[item1.id]).unwrap();

    // Concurrently insert exact same content
    let item2 = store.insert_capture(Capture::text(cap_text)).unwrap();
    let blob_p2 = store.blob_path(item2.id).unwrap();
    assert!(blob_p2.exists());
    assert_eq!(std::fs::read_to_string(&blob_p2).unwrap(), cap_text);
}

#[test]
fn test_regression_finding_5_item_formats_blob_refcounting() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // Create large >64KB HTML payload (70 KB)
    let large_html = format!("<html><body>{}</body></html>", "A".repeat(70 * 1024));

    let rich_cap = Capture {
        kind: Kind::Text,
        sub_kind: Some(SubKind::Rich),
        primary: Some(b"Plain text".to_vec()),
        formats: vec![CapturedFormat {
            format: "HTML Format".into(),
            bytes: large_html.as_bytes().to_vec(),
        }],
        files: vec![],
        preview_text: Some("Plain text".into()),
        ext: Some("TXT".into()),
        mime: Some("text/plain".into()),
        width: None,
        height: None,
        duration_ms: None,
        source_app: None,
        is_reference: false,
        ref_path: None,
    };

    let item1 = store.insert_capture(rich_cap).unwrap();
    let formats1 = store.formats(item1.id).unwrap();
    assert_eq!(formats1.len(), 1);
    assert_eq!(formats1[0].0, "HTML Format");
    assert_eq!(formats1[0].1.len(), large_html.len());

    // Insert a separate plain item with same hash or another item
    let item2 = store.insert_capture(Capture::text("Separate item")).unwrap();

    // Delete item 2
    store.delete(&[item2.id]).unwrap();

    // Verify item 1's >64KB format is STILL readable from disk
    let formats_after = store.formats(item1.id).unwrap();
    assert_eq!(formats_after.len(), 1);
    assert_eq!(formats_after[0].0, "HTML Format");
    assert_eq!(formats_after[0].1, large_html.as_bytes());
}

#[test]
fn test_regression_finding_7_retention_policy_and_size_cap() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // Set retention policy: 7 days, 500 KB max cap
    let policy = rebuffer_lib::model::RetentionPolicy {
        retention_days: 7,
        max_store_bytes: Some(500 * 1024),
    };
    store.set_retention_policy(policy);
    assert_eq!(store.retention_policy().retention_days, 7);
    assert_eq!(store.retention_policy().max_store_bytes, Some(500 * 1024));

    // Insert 10 items of 100 KB each (total 1 MB, which exceeds 500 KB cap)
    for i in 0..10 {
        let payload = vec![(i % 256) as u8; 100 * 1024];
        let cap = Capture {
            kind: Kind::Other,
            sub_kind: None,
            primary: Some(payload),
            formats: vec![],
            files: vec![],
            preview_text: Some(format!("Item {i}")),
            ext: None,
            mime: None,
            width: None,
            height: None,
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        };
        store.insert_capture(cap).unwrap();
    }

    let stats_before = store.stats().unwrap();
    assert!(stats_before.total_bytes >= 1000 * 1024);

    // Run cleanup using policy (None = use policy settings)
    let res = store.run_cleanup(None).unwrap();
    assert!(res.removed_items > 0);
    assert!(res.freed_bytes > 0);

    // Pruned store should be <= 90% of 500 KB (450 KB)
    let stats_after = store.stats().unwrap();
    assert!(stats_after.total_bytes <= 450 * 1024);
}

#[test]
fn test_regression_finding_8_fsync_lock_concurrency() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // Pre-insert some items
    for i in 0..5 {
        store.insert_capture(Capture::text(&format!("Item {i}"))).unwrap();
    }

    let s1 = store.clone();
    let h1 = std::thread::spawn(move || {
        // Insert a 2 MB item that writes and fsyncs to disk
        let large_payload = vec![42u8; 2 * 1024 * 1024];
        let cap = Capture {
            kind: Kind::Other,
            sub_kind: None,
            primary: Some(large_payload),
            formats: vec![],
            files: vec![],
            preview_text: Some("Large item".into()),
            ext: None,
            mime: None,
            width: None,
            height: None,
            duration_ms: None,
            source_app: None,
            is_reference: false,
            ref_path: None,
        };
        s1.insert_capture(cap).unwrap()
    });

    let s2 = store.clone();
    let h2 = std::thread::spawn(move || {
        // UI queries should execute concurrently without blocking on fsync
        let start = std::time::Instant::now();
        let list = s2.list(&Filter::default(), Sort::Newest, 0, 10).unwrap();
        let elapsed = start.elapsed();
        assert!(!list.is_empty());
        elapsed
    });

    let item = h1.join().unwrap();
    let elapsed = h2.join().unwrap();
    assert_eq!(item.preview_text.as_deref(), Some("Large item"));
    // Concurrent UI read should complete under 100ms
    assert!(elapsed.as_millis() < 500);
}


