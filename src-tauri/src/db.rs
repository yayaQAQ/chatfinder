use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct DbState(pub Mutex<Connection>);

pub fn db_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("chatvault.db")
}

pub fn open(path: &PathBuf) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            platform TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            summary TEXT NOT NULL DEFAULT '',
            url TEXT,
            created_at TEXT,
            updated_at TEXT,
            message_count INTEGER NOT NULL DEFAULT 0,
            models TEXT NOT NULL DEFAULT '',
            content_hash TEXT NOT NULL DEFAULT '',
            imported_at TEXT NOT NULL,
            import_batch_id TEXT,
            cwd TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            sender TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            created_at TEXT,
            seq INTEGER NOT NULL,
            kind TEXT NOT NULL DEFAULT 'text',
            model TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at);
        CREATE INDEX IF NOT EXISTS idx_conversations_created ON conversations(created_at);
        CREATE INDEX IF NOT EXISTS idx_conversations_platform ON conversations(platform);

        CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
            ref_id UNINDEXED,
            conversation_id UNINDEXED,
            kind UNINDEXED,
            text
        );

        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            color TEXT NOT NULL DEFAULT '#6366f1'
        );

        CREATE TABLE IF NOT EXISTS favorites (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            message_id TEXT,
            selected_text TEXT NOT NULL,
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_favorites_conv ON favorites(conversation_id);
        CREATE INDEX IF NOT EXISTS idx_favorites_created ON favorites(created_at);

        CREATE TABLE IF NOT EXISTS favorite_tags (
            favorite_id TEXT NOT NULL REFERENCES favorites(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (favorite_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS import_batches (
            id TEXT PRIMARY KEY,
            source_file TEXT NOT NULL,
            platform TEXT NOT NULL,
            imported_at TEXT NOT NULL,
            added_count INTEGER NOT NULL DEFAULT 0,
            updated_count INTEGER NOT NULL DEFAULT 0,
            skipped_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS embeddings (
            message_id TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            vec BLOB NOT NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL DEFAULT ''
        );
        ",
    )?;

    // Migrations for DBs created before a column existed:
    //  - `messages.kind`  — agent sessions used to drop tool_use/tool_result/thinking entirely.
    //  - `messages.model` / `conversations.models` — the model behind each turn.
    // Rows written before the column existed keep the default until the source
    // is re-imported; `content_hash` covers the model, so a re-scan refreshes them.
    //  - `conversations.cwd` — the working directory of an agent session. It used
    //    to be squatted into `summary`, a field that ZIP-imported Claude rows use
    //    for an AI-written prose summary instead. Overloading one column with two
    //    unrelated meanings is fine as long as only the UI reads it, but the MCP
    //    server exposes directories to outside agents, so it gets its own column.
    //    `summary` is still written with the same value for now — the detail view
    //    reads it to offer "resume in terminal".
    add_column_if_missing(&conn, "conversations", "cwd", "TEXT NOT NULL DEFAULT ''")?;
    conn.execute(
        "UPDATE conversations SET cwd = summary
         WHERE cwd = '' AND platform IN ('claude-code', 'codex') AND summary LIKE '/%'",
        [],
    )?;
    // Indexed here rather than in the batch above: on an upgrade the column is
    // only added by the ALTER TABLE on the line before this.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_conversations_cwd ON conversations(cwd) WHERE cwd != '';",
    )?;

    add_column_if_missing(&conn, "messages", "kind", "TEXT NOT NULL DEFAULT 'text'")?;
    add_column_if_missing(&conn, "messages", "model", "TEXT")?;
    add_column_if_missing(&conn, "conversations", "models", "TEXT NOT NULL DEFAULT ''")?;

    Ok(conn)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let exists = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        names.iter().any(|n| n == column)
    };
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
    }
    Ok(())
}
