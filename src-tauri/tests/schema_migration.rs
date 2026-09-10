//! Opening a database created by an older build must upgrade it in place.
//!
//! Every other test starts from an empty file, where `CREATE TABLE` carries the
//! current columns and migrations are no-ops — so the upgrade path needs a
//! database that genuinely predates them, built here by hand.

use app_lib::test_support::open_for_test;

fn old_schema_db(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("chatvault_migrate_{name}_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn = rusqlite::Connection::open(&path).unwrap();
    // The shape shipped before `cwd`, `kind` and `model` existed: agent sessions
    // squatted their working directory in `summary`.
    conn.execute_batch(
        "
        CREATE TABLE conversations (
            id TEXT PRIMARY KEY,
            platform TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '',
            url TEXT,
            created_at TEXT,
            updated_at TEXT,
            message_count INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT NOT NULL DEFAULT '',
            imported_at TEXT NOT NULL,
            import_batch_id TEXT
        );
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            sender TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            created_at TEXT,
            seq INTEGER NOT NULL
        );
        INSERT INTO conversations (id, platform, title, summary, imported_at)
            VALUES ('cc:1', 'claude-code', 'Session', '/Users/dev/code/proj', '2026-01-01T00:00:00Z');
        INSERT INTO conversations (id, platform, title, summary, imported_at)
            VALUES ('cx:1', 'codex', 'Session', '/Users/dev/code/proj', '2026-01-01T00:00:00Z');
        INSERT INTO conversations (id, platform, title, summary, imported_at)
            VALUES ('web:1', 'claude', 'Web chat', '**Conversation Overview**

The user asked about…', '2026-01-01T00:00:00Z');
        ",
    )
    .unwrap();
    path
}

#[test]
fn upgrades_a_database_that_predates_the_cwd_column() {
    let path = old_schema_db("cwd");
    let conn = open_for_test(&path).expect("opening an old database must migrate, not fail");

    let backfilled: Vec<(String, String)> = conn
        .prepare("SELECT id, cwd FROM conversations WHERE cwd != '' ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        backfilled,
        vec![
            ("cc:1".to_string(), "/Users/dev/code/proj".to_string()),
            ("cx:1".to_string(), "/Users/dev/code/proj".to_string()),
        ],
        "agent sessions should carry their directory over from summary"
    );

    // A Claude web export's prose summary is not a path and must not be treated
    // as one — that overload is the whole reason for the separate column.
    let web_cwd: String = conn
        .query_row("SELECT cwd FROM conversations WHERE id = 'web:1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(web_cwd, "");

    let has_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_conversations_cwd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_index, 1, "the cwd index must be created on upgrade too");

    // Reopening is idempotent — migrations run on every launch.
    drop(conn);
    let conn = open_for_test(&path).expect("second open");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversations WHERE cwd != ''", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}
