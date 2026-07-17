use app_lib::test_support::{import_for_test, open_for_test};

#[test]
fn imports_real_claude_export_and_dedupes_on_reimport() {
    let zip_path = "/Users/a1-6/PycharmProjects/remake_history/data-2f47762c-a31e-4d63-89d3-e107d26a1518-1782326928-6a155761-batch-0000.zip";
    let db_path = std::env::temp_dir().join("chatvault_test_import.db");
    let _ = std::fs::remove_file(&db_path);

    let mut conn = open_for_test(&db_path).expect("open db");

    let first = import_for_test(&mut conn, zip_path, "batch-1", None).expect("first import");
    assert_eq!(first.platform, "claude");
    assert!(first.added > 0, "expected new conversations on first import");
    assert_eq!(first.skipped, 0, "nothing should be skipped on first import");

    let conv_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(conv_count, first.added);

    let msg_count: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
    assert!(msg_count > 0, "expected messages to be imported");

    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))
        .unwrap();
    assert!(fts_count >= msg_count);

    // Re-importing the same file should dedupe everything.
    let second = import_for_test(&mut conn, zip_path, "batch-2", None).expect("second import");
    assert_eq!(second.added, 0, "no new conversations expected on reimport");
    assert_eq!(second.updated, 0, "nothing changed, so no updates expected");
    assert_eq!(second.skipped, first.added, "everything should be skipped as duplicate");

    let conv_count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(conv_count_after, conv_count, "dedupe must not create duplicate rows");

    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_index WHERE text MATCH 'Claude*'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!(
        "conversations={conv_count_after} messages={msg_count} fts_rows={fts_count} claude_hits={hits}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn ranked_search_query_executes_without_error() {
    let zip_path = "/Users/a1-6/PycharmProjects/remake_history/data-2f47762c-a31e-4d63-89d3-e107d26a1518-1782326928-6a155761-batch-0000.zip";
    let db_path = std::env::temp_dir().join("chatvault_test_rank.db");
    let _ = std::fs::remove_file(&db_path);
    let mut conn = open_for_test(&db_path).expect("open db");
    import_for_test(&mut conn, zip_path, "batch-rank", None).expect("import");

    let mut stmt = conn
        .prepare(
            "SELECT c.id FROM search_index si JOIN conversations c ON c.id = si.conversation_id
             WHERE si.text MATCH 'Claude*' ORDER BY bm25(search_index) ASC LIMIT 50",
        )
        .unwrap();
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(!ids.is_empty(), "expected ranked results for a common term");
    println!("top ranked conversation ids: {ids:?}");

    let _ = std::fs::remove_file(&db_path);
}
