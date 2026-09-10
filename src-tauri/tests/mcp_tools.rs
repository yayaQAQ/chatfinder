//! Drives the MCP tool table against a real SQLite file, covering the SQL and
//! the directory scoping that the unit tests in `mcp.rs` cannot reach.

use app_lib::test_support::{mcp_tool_for_test as tool, open_for_test};
use serde_json::json;

fn fixture_db(name: &str) -> rusqlite::Connection {
    let path = std::env::temp_dir().join(format!("chatvault_mcp_{name}_{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn = open_for_test(&path).expect("open db");
    conn.execute_batch(
        r#"
        INSERT INTO conversations (id, platform, title, cwd, message_count, imported_at, created_at, updated_at)
          VALUES ('cc:1', 'claude-code', 'Migration work', '/Users/dev/code/proj', 3,
                  '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-03-01T00:00:00Z');
        INSERT INTO conversations (id, platform, title, cwd, message_count, imported_at, created_at, updated_at)
          VALUES ('cx:1', 'codex', 'Sub-module work', '/Users/dev/code/proj/app', 1,
                  '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z');
        INSERT INTO conversations (id, platform, title, cwd, message_count, imported_at, created_at, updated_at)
          VALUES ('cc:2', 'claude-code', 'Unrelated', '/Users/dev/code/other', 1,
                  '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
        INSERT INTO conversations (id, platform, title, message_count, imported_at, created_at, updated_at)
          VALUES ('web:1', 'chatgpt', 'Web chat about proj', 1,
                  '2026-01-01T00:00:00Z', '2026-01-05T00:00:00Z', '2026-01-05T00:00:00Z');

        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m1', 'cc:1', 'human', 'how do I run the database migration', 0, 'text');
        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m2', 'cc:1', 'assistant', 'run the migration with cargo', 1, 'text');
        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m3', 'cc:1', 'assistant', 'tool output noise', 2, 'tool_result');
        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m4', 'cx:1', 'assistant', 'migration touched the app folder', 0, 'text');
        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m5', 'cc:2', 'assistant', 'migration in a different project', 0, 'text');
        INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
          VALUES ('m6', 'web:1', 'assistant', '重构方案已经确定，先做数据库迁移', 0, 'text');

        INSERT INTO search_index (ref_id, conversation_id, kind, text)
          VALUES ('m1', 'cc:1', 'message', 'how do I run the database migration');
        INSERT INTO search_index (ref_id, conversation_id, kind, text)
          VALUES ('m2', 'cc:1', 'message', 'run the migration with cargo');
        INSERT INTO search_index (ref_id, conversation_id, kind, text)
          VALUES ('m4', 'cx:1', 'message', 'migration touched the app folder');
        INSERT INTO search_index (ref_id, conversation_id, kind, text)
          VALUES ('m5', 'cc:2', 'message', 'migration in a different project');
        INSERT INTO search_index (ref_id, conversation_id, kind, text)
          VALUES ('m6', 'web:1', 'message', '重构方案已经确定，先做数据库迁移');
        "#,
    )
    .expect("seed");
    conn
}

#[test]
fn overview_resolves_a_subdirectory_to_every_project_directory_above_it() {
    let conn = fixture_db("overview_up");
    // Two levels below where a session was recorded — the case exact matching
    // gets wrong. Both `/proj` and `/proj/app` are real ancestors of it.
    let out = tool(
        &conn,
        "overview",
        &json!({ "cwd": "/Users/dev/code/proj/app/src-tauri" }),
    )
    .expect("overview");

    let current = &out["current_directory"];
    assert!(current["exact"].is_null(), "nothing was recorded at that exact path");
    let ancestors: Vec<&str> = current["ancestors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(ancestors, vec!["/Users/dev/code/proj", "/Users/dev/code/proj/app"]);
    assert_eq!(current["descendants"].as_array().unwrap().len(), 0);

    // The web chat has no directory at all and must be accounted for, or an
    // agent will assume cwd scoping covers the whole library.
    assert_eq!(out["directory_coverage"]["conversations_without_directory"], 1);
    assert_eq!(out["directory_coverage"]["distinct_directories"], 3);
    // The fallback keyword names the project, not the leaf directory.
    let hint = current["hint"].as_str().unwrap();
    assert!(hint.contains("\"proj\""), "got: {hint}");

    // Directories are ordered by recency, not volume.
    let dirs = out["directories"].as_array().unwrap();
    assert_eq!(dirs[0]["path"], "/Users/dev/code/proj");
}

#[test]
fn overview_from_a_project_root_finds_sessions_recorded_in_submodules() {
    let conn = fixture_db("overview_down");
    let out = tool(&conn, "overview", &json!({ "cwd": "/Users/dev/code/proj" })).expect("overview");
    let current = &out["current_directory"];
    assert_eq!(current["exact"]["path"], "/Users/dev/code/proj");
    let descendants: Vec<&str> = current["descendants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(descendants, vec!["/Users/dev/code/proj/app"]);
    assert_eq!(current["ancestors"].as_array().unwrap().len(), 0);
}

#[test]
fn search_scoped_to_a_directory_covers_the_whole_tree_and_excludes_other_projects() {
    let conn = fixture_db("search_scope");
    let out = tool(
        &conn,
        "search",
        &json!({ "query": "migration", "cwd": "/Users/dev/code/proj" }),
    )
    .expect("search");

    let ids: Vec<&str> = out["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["conversation_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"cc:1"), "the project root session should match");
    assert!(ids.contains(&"cx:1"), "a session in a subdirectory belongs to the same project");
    assert!(!ids.contains(&"cc:2"), "a different project must not leak in");
    assert!(!ids.contains(&"web:1"), "a chat with no directory is out of scope");
}

#[test]
fn a_directory_with_no_history_says_so_instead_of_silently_returning_nothing() {
    let conn = fixture_db("empty_scope");
    let out = tool(
        &conn,
        "search",
        &json!({ "query": "migration", "cwd": "/Users/dev/somewhere/else" }),
    )
    .expect("search");
    assert_eq!(out["hits"].as_array().unwrap().len(), 0);
    let notes = out["notes"].as_array().unwrap();
    assert!(
        notes.iter().any(|n| n.as_str().unwrap().contains("No recorded working directory")),
        "expected an explanatory note, got {notes:?}"
    );
}

#[test]
fn chinese_queries_fall_back_to_substring_and_actually_find_the_message() {
    let conn = fixture_db("cjk");
    // "方案" is mid-token for FTS5's unicode61 tokenizer, so the prefix query
    // the app uses cannot match it. The substring path must.
    let out = tool(&conn, "search", &json!({ "query": "方案" })).expect("search");
    assert_eq!(out["mode_used"], "substring");
    let hits = out["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["conversation_id"], "web:1");
    assert!(hits[0]["snippet"].as_str().unwrap().contains("【方案】"));
}

#[test]
fn a_whole_word_match_wins_over_a_prefix_match() {
    let conn = fixture_db("word_first");
    // "cargo" appears as a whole word in cc:1 and only as a prefix inside
    // "cargotown" here — the whole-word hit is the one that should come back.
    conn.execute_batch(
        "INSERT INTO conversations (id, platform, title, message_count, imported_at, updated_at)
           VALUES ('noise:1', 'chatgpt', 'Noise', 1, '2026-01-01T00:00:00Z', '2026-06-01T00:00:00Z');
         INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
           VALUES ('m9', 'noise:1', 'assistant', 'cargotown cargotastic', 0, 'text');
         INSERT INTO search_index (ref_id, conversation_id, kind, text)
           VALUES ('m9', 'noise:1', 'message', 'cargotown cargotastic');",
    )
    .unwrap();

    let out = tool(&conn, "search", &json!({ "query": "cargo" })).expect("search");
    assert_eq!(out["mode_used"], "fts");
    let ids: Vec<&str> = out["hits"].as_array().unwrap().iter()
        .map(|h| h["conversation_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["cc:1"], "prefix noise must not crowd out the real match");

    // A term that only exists as a prefix still falls back and finds it.
    let out = tool(&conn, "search", &json!({ "query": "cargota" })).expect("search");
    assert_eq!(out["mode_used"], "fts_prefix");
    assert_eq!(out["hits"][0]["conversation_id"], "noise:1");
}

#[test]
fn the_suggested_keyword_is_the_project_name_not_the_current_leaf_folder() {
    let conn = fixture_db("hint");
    let out = tool(
        &conn,
        "overview",
        &json!({ "cwd": "/Users/dev/code/proj/app/src-tauri/src" }),
    )
    .expect("overview");
    let hint = out["current_directory"]["hint"].as_str().unwrap();
    assert!(hint.contains("\"proj\""), "expected the project name, got: {hint}");
    assert!(!hint.contains("\"src\""));
}

#[test]
fn search_can_be_narrowed_to_what_the_user_typed() {
    let conn = fixture_db("sender");
    let out = tool(&conn, "search", &json!({ "query": "migration", "sender": "human" }))
        .expect("search");
    let hits = out["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["message_id"], "m1");
    assert_eq!(hits[0]["sender"], "human");
}

#[test]
fn get_conversation_paginates_and_can_skip_tool_noise() {
    let conn = fixture_db("get_conv");
    let all = tool(&conn, "get_conversation", &json!({ "id": "cc:1" })).expect("get");
    assert_eq!(all["pagination"]["total_matching"], 3);

    let text_only = tool(
        &conn,
        "get_conversation",
        &json!({ "id": "cc:1", "kinds": ["text"] }),
    )
    .expect("get");
    assert_eq!(text_only["pagination"]["total_matching"], 2);
    assert_eq!(text_only["messages"].as_array().unwrap().len(), 2);

    let page = tool(
        &conn,
        "get_conversation",
        &json!({ "id": "cc:1", "limit": 1, "offset": 0 }),
    )
    .expect("get");
    assert_eq!(page["pagination"]["has_more"], true);
    assert_eq!(page["messages"].as_array().unwrap().len(), 1);

    assert!(tool(&conn, "get_conversation", &json!({ "id": "nope" })).is_err());
}

#[test]
fn long_messages_are_truncated_so_one_tool_result_cannot_flood_the_context() {
    let conn = fixture_db("truncate");
    conn.execute(
        "INSERT INTO messages (id, conversation_id, sender, text, seq, kind)
         VALUES ('big', 'cc:1', 'assistant', ?1, 9, 'tool_result')",
        [&"x".repeat(50_000)],
    )
    .unwrap();

    let out = tool(&conn, "get_conversation", &json!({ "id": "cc:1", "offset": 3 })).expect("get");
    assert_eq!(out["truncated_messages"], 1);
    let text = out["messages"][0]["text"].as_str().unwrap();
    assert!(text.chars().count() < 2_200, "got {} chars", text.chars().count());
    assert!(text.contains("more characters"));
}

#[test]
fn message_context_returns_the_neighbours_of_a_search_hit() {
    let conn = fixture_db("context");
    let out = tool(
        &conn,
        "get_message_context",
        &json!({ "message_id": "m2", "before": 1, "after": 1 }),
    )
    .expect("context");
    assert_eq!(out["target_seq"], 1);
    let seqs: Vec<i64> = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["seq"].as_i64().unwrap())
        .collect();
    assert_eq!(seqs, vec![0, 1, 2]);
    assert_eq!(out["conversation"]["id"], "cc:1");
}

#[test]
fn list_conversations_reports_more_pages_without_returning_them() {
    let conn = fixture_db("list");
    let out = tool(&conn, "list_conversations", &json!({ "limit": 2 })).expect("list");
    assert_eq!(out["conversations"].as_array().unwrap().len(), 2);
    assert_eq!(out["has_more"], true);
    // Newest first: cc:1 was updated in March.
    assert_eq!(out["conversations"][0]["id"], "cc:1");
}

#[test]
fn unknown_tools_are_rejected_by_name() {
    let conn = fixture_db("unknown");
    let err = tool(&conn, "delete_everything", &json!({})).unwrap_err();
    assert!(err.contains("unknown tool"));
}
