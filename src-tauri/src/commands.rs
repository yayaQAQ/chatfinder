use crate::agent_scan::{self, AgentSource};
use crate::db::DbState;
use crate::embed;
use crate::import;
use crate::keywords;
use crate::models::{
    ConversationSummary, EmbeddingStats, FavoriteRow, ImportBatchRow, ImportSummary, MessageRow, SearchHit, TagRow,
};
use rusqlite::params;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

#[derive(Serialize, Clone)]
pub struct ImportProgress {
    current: usize,
    total: usize,
    phase: String, // "parse" | "db"
}

// Progress is sent over a dedicated per-invocation Channel rather than a
// broadcast app.emit(): app-wide events go through a shared listeners mutex
// that, under contention from a concurrent background thread, can queue an
// event as "pending" and flush it only after a *later* event already got
// delivered — visibly reordering the stream (progress jumps to ~100% then
// drops back down). Channel::send() writes straight to the webview with no
// shared lock, so ordering is guaranteed.
#[tauri::command]
pub async fn import_zip_file(
    app: tauri::AppHandle,
    zip_path: String,
    on_progress: Channel<ImportProgress>,
) -> Result<ImportSummary, String> {
    let batch_id = Uuid::new_v4().to_string();
    // Signal that parsing has begun (total unknown yet)
    on_progress.send(ImportProgress { current: 0, total: 0, phase: "parse".to_string() }).ok();

    // ZIP parsing + DB writes are CPU/IO heavy; a plain sync command runs on
    // Tauri's main thread and would freeze the window (OS shows a spinning
    // wait cursor) for the whole import. Run it on a blocking thread instead.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DbState>();
        let mut conn = state.0.lock().map_err(|e| e.to_string())?;
        import::import_zip(&mut conn, &zip_path, &batch_id, Some(&move |current, total| {
            on_progress.send(ImportProgress { current, total, phase: "db".to_string() }).ok();
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sort_clause(sort: &str) -> &'static str {
    match sort {
        "oldest"          => "COALESCE(updated_at, created_at, imported_at) ASC",
        "most_messages"   => "message_count DESC",
        "fewest_messages" => "message_count ASC",
        _                 => "COALESCE(updated_at, created_at, imported_at) DESC",
    }
}

#[tauri::command]
pub fn list_conversations(
    state: State<DbState>,
    query: Option<String>,
    platform: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    min_messages: Option<i64>,
    max_messages: Option<i64>,
    sort: Option<String>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ConversationSummary>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    let plat_filter = platform.unwrap_or_default();
    let trimmed_query = query.unwrap_or_default().trim().to_string();
    let df = date_from.unwrap_or_default();
    let dt = date_to.unwrap_or_default();
    let min_msg = min_messages.unwrap_or(0);
    let max_msg = max_messages.unwrap_or(0); // 0 = no upper limit
    let sort_str = sort.as_deref().unwrap_or("newest");
    let order = sort_clause(sort_str);

    let mut rows_out = Vec::new();

    if trimmed_query.is_empty() {
        let sql = format!(
            "SELECT id, platform, title, summary, url, created_at, updated_at, message_count
             FROM conversations
             WHERE (?1 = '' OR platform = ?1)
               AND (?2 = '' OR DATE(COALESCE(updated_at, created_at, imported_at)) >= ?2)
               AND (?3 = '' OR DATE(COALESCE(updated_at, created_at, imported_at)) <= ?3)
               AND (?4 = 0 OR message_count >= ?4)
               AND (?5 = 0 OR message_count <= ?5)
             ORDER BY {order}
             LIMIT ?6 OFFSET ?7"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![plat_filter, df, dt, min_msg, max_msg, limit, offset])
            .map_err(|e| e.to_string())?;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            rows_out.push(ConversationSummary {
                id: row.get(0).map_err(|e| e.to_string())?,
                platform: row.get(1).map_err(|e| e.to_string())?,
                title: row.get(2).map_err(|e| e.to_string())?,
                summary: row.get(3).map_err(|e| e.to_string())?,
                url: row.get(4).map_err(|e| e.to_string())?,
                created_at: row.get(5).map_err(|e| e.to_string())?,
                updated_at: row.get(6).map_err(|e| e.to_string())?,
                message_count: row.get(7).map_err(|e| e.to_string())?,
            });
        }
    } else {
        let fts_query = format!("{}*", trimmed_query.replace('"', " "));
        let sql = format!(
            "SELECT c.id, c.platform, c.title, c.summary, c.url, c.created_at, c.updated_at, c.message_count
             FROM search_index si
             JOIN conversations c ON c.id = si.conversation_id
             WHERE si.text MATCH ?1
               AND (?2 = '' OR c.platform = ?2)
               AND (?3 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) >= ?3)
               AND (?4 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) <= ?4)
               AND (?5 = 0 OR c.message_count >= ?5)
               AND (?6 = 0 OR c.message_count <= ?6)
             ORDER BY bm25(search_index) ASC
             LIMIT 500"
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![fts_query, plat_filter, df, dt, min_msg, max_msg])
            .map_err(|e| e.to_string())?;
        let mut seen = std::collections::HashSet::new();
        let mut ranked = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(0).map_err(|e| e.to_string())?;
            if !seen.insert(id.clone()) { continue; }
            ranked.push(ConversationSummary {
                id,
                platform: row.get(1).map_err(|e| e.to_string())?,
                title: row.get(2).map_err(|e| e.to_string())?,
                summary: row.get(3).map_err(|e| e.to_string())?,
                url: row.get(4).map_err(|e| e.to_string())?,
                created_at: row.get(5).map_err(|e| e.to_string())?,
                updated_at: row.get(6).map_err(|e| e.to_string())?,
                message_count: row.get(7).map_err(|e| e.to_string())?,
            });
        }
        let start = offset.max(0) as usize;
        let end = (start + limit.max(0) as usize).min(ranked.len());
        rows_out = if start < ranked.len() { ranked[start..end].to_vec() } else { vec![] };
    }

    Ok(rows_out)
}

#[tauri::command]
pub fn count_conversations(state: State<DbState>) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_conversation(
    state: State<DbState>,
    id: String,
) -> Result<(ConversationSummary, Vec<MessageRow>), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let conv = conn
        .query_row(
            "SELECT id, platform, title, summary, url, created_at, updated_at, message_count FROM conversations WHERE id = ?1",
            params![id],
            |row| {
                Ok(ConversationSummary {
                    id: row.get(0)?,
                    platform: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    url: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    message_count: row.get(7)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT id, conversation_id, sender, text, created_at, seq FROM messages WHERE conversation_id = ?1 ORDER BY seq ASC")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![id]).map_err(|e| e.to_string())?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        messages.push(MessageRow {
            id: row.get(0).map_err(|e| e.to_string())?,
            conversation_id: row.get(1).map_err(|e| e.to_string())?,
            sender: row.get(2).map_err(|e| e.to_string())?,
            text: row.get(3).map_err(|e| e.to_string())?,
            created_at: row.get(4).map_err(|e| e.to_string())?,
            seq: row.get(5).map_err(|e| e.to_string())?,
        });
    }

    Ok((conv, messages))
}

/// Claude Code and Codex sessions store their working directory in
/// `conversations.summary` (see agent_scan.rs) — this is the only field that
/// currently carries it, ZIP-imported platforms always leave it empty. Used
/// to group every session under a project directory regardless of which
/// tool produced it.
#[tauri::command]
pub fn list_conversations_by_path(state: State<DbState>, path: String) -> Result<Vec<ConversationSummary>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, platform, title, summary, url, created_at, updated_at, message_count
             FROM conversations
             WHERE summary = ?1 AND summary != ''
             ORDER BY COALESCE(updated_at, created_at) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![path], |row| {
            Ok(ConversationSummary {
                id: row.get(0)?,
                platform: row.get(1)?,
                title: row.get(2)?,
                summary: row.get(3)?,
                url: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                message_count: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Removes everything belonging to `conversation_id` except the row itself,
/// which the caller deletes afterward. `messages`, `favorites`, and
/// `favorite_tags` all cascade via `ON DELETE CASCADE` FKs (see db.rs), but
/// `search_index` (FTS5 has no FK support) and `embeddings` (no FK defined,
/// keyed by message_id) don't — those need explicit cleanup, mirroring the
/// same cleanup persist_conversations already does when a reimport changes a
/// conversation's content.
fn delete_conversation_dependents(tx: &rusqlite::Transaction, conversation_id: &str) -> Result<(), String> {
    tx.execute(
        "DELETE FROM embeddings WHERE message_id IN (SELECT id FROM messages WHERE conversation_id = ?1)",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM search_index WHERE conversation_id = ?1", params![conversation_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_conversation(state: State<DbState>, id: String) -> Result<(), String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    delete_conversation_dependents(&tx, &id)?;
    tx.execute("DELETE FROM conversations WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_import_batches(state: State<DbState>) -> Result<Vec<ImportBatchRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT b.id, b.source_file, b.platform, b.imported_at, b.added_count, b.updated_count, b.skipped_count,
                    (SELECT COUNT(*) FROM conversations c WHERE c.import_batch_id = b.id)
             FROM import_batches b
             ORDER BY b.imported_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ImportBatchRow {
                id: row.get(0)?,
                source_file: row.get(1)?,
                platform: row.get(2)?,
                imported_at: row.get(3)?,
                added_count: row.get(4)?,
                updated_count: row.get(5)?,
                skipped_count: row.get(6)?,
                remaining_conversations: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Deletes every conversation still tagged with this batch, then the batch
/// row itself. Returns how many conversations were removed.
#[tauri::command]
pub fn delete_import_batch(state: State<DbState>, batch_id: String) -> Result<i64, String> {
    let mut conn = state.0.lock().map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;

    let ids: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT id FROM conversations WHERE import_batch_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![batch_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };

    for conv_id in &ids {
        delete_conversation_dependents(&tx, conv_id)?;
    }
    tx.execute(
        "DELETE FROM conversations WHERE import_batch_id = ?1",
        params![batch_id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM import_batches WHERE id = ?1", params![batch_id])
        .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(ids.len() as i64)
}

// Deduped to one hit per conversation (best bm25 rank kept) so a single
// noisy term doesn't let one conversation flood the results list.
#[tauri::command]
pub fn search_all(
    state: State<DbState>,
    query: String,
    platform: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    min_messages: Option<i64>,
    max_messages: Option<i64>,
    role: Option<String>,
) -> Result<Vec<SearchHit>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let plat_filter = platform.unwrap_or_default();
    let df = date_from.unwrap_or_default();
    let dt = date_to.unwrap_or_default();
    let min_msg = min_messages.unwrap_or(0);
    let max_msg = max_messages.unwrap_or(0);
    let role_filter = role.unwrap_or_default();

    let fts_query = format!("{}*", trimmed.replace('"', " "));
    // LEFT JOIN messages: si.ref_id is a message id for kind='message' hits and a
    // conversation id for kind='title' hits, so title hits get sender=NULL and are
    // naturally excluded whenever a role filter is active (a title isn't "user
    // input" or "AI output").
    let mut stmt = conn
        .prepare(
            "SELECT si.ref_id, si.conversation_id, si.kind, snippet(search_index, 3, '【', '】', '…', 12),
                    c.title, c.platform, c.updated_at, c.created_at
             FROM search_index si
             JOIN conversations c ON c.id = si.conversation_id
             LEFT JOIN messages m ON m.id = si.ref_id
             WHERE si.text MATCH ?1
               AND (?2 = '' OR c.platform = ?2)
               AND (?3 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) >= ?3)
               AND (?4 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) <= ?4)
               AND (?5 = 0 OR c.message_count >= ?5)
               AND (?6 = 0 OR c.message_count <= ?6)
               AND (?7 = '' OR m.sender = ?7)
             ORDER BY bm25(search_index) ASC LIMIT 400",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params![fts_query, plat_filter, df, dt, min_msg, max_msg, role_filter])
        .map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let conversation_id: String = row.get(1).map_err(|e| e.to_string())?;
        if !seen.insert(conversation_id.clone()) { continue; }
        hits.push(SearchHit {
            ref_id: row.get(0).map_err(|e| e.to_string())?,
            conversation_id,
            kind: row.get(2).map_err(|e| e.to_string())?,
            snippet: row.get(3).map_err(|e| e.to_string())?,
            conversation_title: row.get(4).map_err(|e| e.to_string())?,
            platform: row.get(5).map_err(|e| e.to_string())?,
            updated_at: row.get(6).map_err(|e| e.to_string())?,
            created_at: row.get(7).map_err(|e| e.to_string())?,
        });
        if hits.len() >= 100 { break; }
    }
    Ok(hits)
}

fn load_tags_for_favorite(conn: &rusqlite::Connection, favorite_id: &str) -> Result<Vec<TagRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.name, t.color FROM tags t
             JOIN favorite_tags ft ON ft.tag_id = t.id
             WHERE ft.favorite_id = ?1 ORDER BY t.name",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![favorite_id]).map_err(|e| e.to_string())?;
    let mut tags = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        tags.push(TagRow {
            id: row.get(0).map_err(|e| e.to_string())?,
            name: row.get(1).map_err(|e| e.to_string())?,
            color: row.get(2).map_err(|e| e.to_string())?,
        });
    }
    Ok(tags)
}

fn get_or_create_tag(conn: &rusqlite::Connection, name: &str) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
        params![name],
    )
    .map_err(|e| e.to_string())?;
    conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get(0))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_favorite(
    state: State<DbState>,
    conversation_id: String,
    message_id: Option<String>,
    selected_text: String,
    note: Option<String>,
    tag_names: Vec<String>,
) -> Result<FavoriteRow, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let note = note.unwrap_or_default();

    conn.execute(
        "INSERT INTO favorites (id, conversation_id, message_id, selected_text, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, conversation_id, message_id, selected_text, note, now],
    )
    .map_err(|e| e.to_string())?;

    let mut names = tag_names;
    if names.is_empty() {
        names = keywords::extract_keywords(&selected_text, 3);
    }
    for name in &names {
        let tag_id = get_or_create_tag(&conn, name)?;
        conn.execute(
            "INSERT OR IGNORE INTO favorite_tags (favorite_id, tag_id) VALUES (?1, ?2)",
            params![id, tag_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let title: String = conn
        .query_row(
            "SELECT title FROM conversations WHERE id = ?1",
            params![conversation_id],
            |r| r.get(0),
        )
        .unwrap_or_default();

    Ok(FavoriteRow {
        id: id.clone(),
        conversation_id,
        conversation_title: title,
        message_id,
        selected_text,
        note,
        created_at: now,
        tags: load_tags_for_favorite(&conn, &id)?,
    })
}

#[tauri::command]
pub fn list_favorites(state: State<DbState>, tag_id: Option<i64>) -> Result<Vec<FavoriteRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;

    let mut stmt = if tag_id.is_some() {
        conn.prepare(
            "SELECT f.id, f.conversation_id, c.title, f.message_id, f.selected_text, f.note, f.created_at
             FROM favorites f JOIN conversations c ON c.id = f.conversation_id
             JOIN favorite_tags ft ON ft.favorite_id = f.id
             WHERE ft.tag_id = ?1
             ORDER BY f.created_at DESC",
        )
    } else {
        conn.prepare(
            "SELECT f.id, f.conversation_id, c.title, f.message_id, f.selected_text, f.note, f.created_at
             FROM favorites f JOIN conversations c ON c.id = f.conversation_id
             ORDER BY f.created_at DESC",
        )
    }
    .map_err(|e| e.to_string())?;

    let mut rows = if let Some(tid) = tag_id {
        stmt.query(params![tid])
    } else {
        stmt.query([])
    }
    .map_err(|e| e.to_string())?;

    let mut favorites = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let fav_id: String = row.get(0).map_err(|e| e.to_string())?;
        favorites.push(FavoriteRow {
            id: fav_id.clone(),
            conversation_id: row.get(1).map_err(|e| e.to_string())?,
            conversation_title: row.get(2).map_err(|e| e.to_string())?,
            message_id: row.get(3).map_err(|e| e.to_string())?,
            selected_text: row.get(4).map_err(|e| e.to_string())?,
            note: row.get(5).map_err(|e| e.to_string())?,
            created_at: row.get(6).map_err(|e| e.to_string())?,
            tags: vec![],
        });
    }
    drop(rows);
    drop(stmt);
    for fav in favorites.iter_mut() {
        fav.tags = load_tags_for_favorite(&conn, &fav.id)?;
    }
    Ok(favorites)
}

#[tauri::command]
pub fn delete_favorite(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM favorites WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_tags(state: State<DbState>) -> Result<Vec<TagRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, color FROM tags ORDER BY name")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut tags = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        tags.push(TagRow {
            id: row.get(0).map_err(|e| e.to_string())?,
            name: row.get(1).map_err(|e| e.to_string())?,
            color: row.get(2).map_err(|e| e.to_string())?,
        });
    }
    Ok(tags)
}

#[tauri::command]
pub fn create_tag(state: State<DbState>, name: String, color: Option<String>) -> Result<TagRow, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let color = color.unwrap_or_else(|| "#6366f1".to_string());
    conn.execute(
        "INSERT INTO tags (name, color) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
        params![name, color],
    )
    .map_err(|e| e.to_string())?;
    let (id, color): (i64, String) = conn
        .query_row("SELECT id, color FROM tags WHERE name = ?1", params![name], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|e| e.to_string())?;
    Ok(TagRow { id, name, color })
}

#[tauri::command]
pub fn set_favorite_tags(state: State<DbState>, favorite_id: String, tag_names: Vec<String>) -> Result<Vec<TagRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM favorite_tags WHERE favorite_id = ?1", params![favorite_id])
        .map_err(|e| e.to_string())?;
    for name in &tag_names {
        let trimmed = name.trim();
        if trimmed.is_empty() { continue; }
        let tag_id = get_or_create_tag(&conn, trimmed)?;
        conn.execute(
            "INSERT OR IGNORE INTO favorite_tags (favorite_id, tag_id) VALUES (?1, ?2)",
            params![favorite_id, tag_id],
        )
        .map_err(|e| e.to_string())?;
    }
    load_tags_for_favorite(&conn, &favorite_id)
}

#[tauri::command]
pub fn suggest_tags(text: String) -> Vec<String> {
    keywords::extract_keywords(&text, 5)
}

#[tauri::command]
pub fn auto_organize_favorites(state: State<DbState>) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.selected_text FROM favorites f
             WHERE NOT EXISTS (SELECT 1 FROM favorite_tags ft WHERE ft.favorite_id = f.id)",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut targets: Vec<(String, String)> = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        targets.push((
            row.get(0).map_err(|e| e.to_string())?,
            row.get(1).map_err(|e| e.to_string())?,
        ));
    }
    drop(rows);
    drop(stmt);

    let mut count = 0i64;
    for (fav_id, text) in targets {
        let kws = keywords::extract_keywords(&text, 3);
        if kws.is_empty() { continue; }
        for kw in &kws {
            let tag_id = get_or_create_tag(&conn, kw)?;
            conn.execute(
                "INSERT OR IGNORE INTO favorite_tags (favorite_id, tag_id) VALUES (?1, ?2)",
                params![fav_id, tag_id],
            )
            .map_err(|e| e.to_string())?;
        }
        count += 1;
    }
    Ok(count)
}

#[tauri::command]
pub fn search_favorites(state: State<DbState>, query: String) -> Result<Vec<FavoriteRow>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let pattern = format!("%{}%", trimmed.to_lowercase());
    let mut stmt = conn
        .prepare(
            "SELECT f.id, f.conversation_id, c.title, f.message_id, f.selected_text, f.note, f.created_at
             FROM favorites f JOIN conversations c ON c.id = f.conversation_id
             WHERE LOWER(f.selected_text) LIKE ?1
                OR LOWER(f.note)          LIKE ?1
                OR LOWER(c.title)         LIKE ?1
             ORDER BY f.created_at DESC
             LIMIT 30",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![pattern]).map_err(|e| e.to_string())?;
    let mut favorites = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        favorites.push(FavoriteRow {
            id:                 row.get(0).map_err(|e| e.to_string())?,
            conversation_id:    row.get(1).map_err(|e| e.to_string())?,
            conversation_title: row.get(2).map_err(|e| e.to_string())?,
            message_id:         row.get(3).map_err(|e| e.to_string())?,
            selected_text:      row.get(4).map_err(|e| e.to_string())?,
            note:               row.get(5).map_err(|e| e.to_string())?,
            created_at:         row.get(6).map_err(|e| e.to_string())?,
            tags:               vec![],
        });
    }
    drop(rows);
    drop(stmt);
    for fav in favorites.iter_mut() {
        fav.tags = load_tags_for_favorite(&conn, &fav.id)?;
    }
    Ok(favorites)
}

// Fix timestamps for all conversations using actual message timestamps
#[tauri::command]
pub fn fix_conversation_timestamps(state: State<DbState>) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let updated = conn.execute(
        "UPDATE conversations
         SET
           created_at = COALESCE(
             created_at,
             (SELECT MIN(m.created_at) FROM messages m WHERE m.conversation_id = conversations.id AND m.created_at IS NOT NULL)
           ),
           updated_at = COALESCE(
             (SELECT m.created_at FROM messages m
              WHERE m.conversation_id = conversations.id AND m.created_at IS NOT NULL
              ORDER BY m.seq DESC LIMIT 1),
             updated_at
           )
         WHERE EXISTS (SELECT 1 FROM messages m WHERE m.conversation_id = conversations.id)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(updated as i64)
}

// ─── Embedding / semantic-search commands ────────────────────────────────────

#[tauri::command]
pub async fn test_embed_connection(
    api_url: String,
    model: String,
    api_key: String,
) -> Result<String, String> {
    let vecs = embed::call_embed_api(&api_url, &model, &api_key, vec!["连接测试".to_string()]).await?;
    let dim = vecs.first().map(|v| v.len()).unwrap_or(0);
    Ok(format!("连接成功，向量维度 {dim}"))
}

#[tauri::command]
pub fn get_setting(state: State<DbState>, key: String) -> Result<Option<String>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .ok())
}

#[tauri::command]
pub fn set_setting(state: State<DbState>, key: String, value: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_embedding_stats(state: State<DbState>) -> Result<EmbeddingStats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages WHERE length(text) > 10", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let indexed: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let model: Option<String> = conn
        .query_row("SELECT model FROM embeddings LIMIT 1", [], |r| r.get(0))
        .ok();
    Ok(EmbeddingStats { total_messages: total, indexed_messages: indexed, model })
}

#[derive(Serialize, Clone)]
struct EmbedProgress {
    current: usize,
    total: usize,
}

#[tauri::command]
pub async fn generate_embeddings(
    app: tauri::AppHandle,
    state: tauri::State<'_, DbState>,
    api_url: String,
    model: String,
    api_key: String,
) -> Result<EmbeddingStats, String> {
    const BATCH: usize = 16;
    const MAX_CHARS: usize = 2000;

    // Collect unindexed messages without holding the lock across awaits
    let unindexed: Vec<(String, String)> = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.text FROM messages m
                 LEFT JOIN embeddings e ON e.message_id = m.id
                 WHERE e.message_id IS NULL AND length(m.text) > 10",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: String = row.get(0).map_err(|e| e.to_string())?;
            let text: String = row.get(1).map_err(|e| e.to_string())?;
            // Truncate to avoid token-limit errors
            let truncated = if text.len() > MAX_CHARS {
                text.chars().take(MAX_CHARS).collect()
            } else {
                text
            };
            out.push((id, truncated));
        }
        out
        // MutexGuard dropped here
    };

    let total = unindexed.len();
    let mut done = 0usize;

    for chunk in unindexed.chunks(BATCH) {
        let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        let ids: Vec<String> = chunk.iter().map(|(id, _)| id.clone()).collect();

        // API call — no lock held
        let vecs = embed::call_embed_api(&api_url, &model, &api_key, texts).await?;

        // Store in DB — hold lock briefly
        {
            let mut conn = state.0.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            for (id, vec) in ids.iter().zip(vecs.iter()) {
                let blob = embed::vec_to_blob(vec);
                tx.execute(
                    "INSERT OR REPLACE INTO embeddings (message_id, model, vec) VALUES (?1, ?2, ?3)",
                    params![id, model, blob],
                )
                .map_err(|e| e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            // MutexGuard dropped here
        }

        done += vecs.len();
        app.emit("embed:progress", EmbedProgress { current: done, total }).ok();
    }

    get_embedding_stats(state)
}

#[tauri::command]
pub async fn semantic_search(
    state: tauri::State<'_, DbState>,
    query: String,
    api_url: String,
    model: String,
    api_key: String,
    limit: usize,
    platform: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
    min_messages: Option<i64>,
    max_messages: Option<i64>,
    role: Option<String>,
) -> Result<Vec<SearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let plat_filter = platform.unwrap_or_default();
    let df = date_from.unwrap_or_default();
    let dt = date_to.unwrap_or_default();
    let min_msg = min_messages.unwrap_or(0);
    let max_msg = max_messages.unwrap_or(0);
    let role_filter = role.unwrap_or_default();

    // 1. Embed the query — no lock held during network call
    let vecs = embed::call_embed_api(&api_url, &model, &api_key, vec![query]).await?;
    let query_vec = vecs.into_iter().next().ok_or("无嵌入结果".to_string())?;

    // 2. Load embeddings, restricted to the requested sender — hold lock briefly
    let candidates: Vec<(String, String, Vec<f32>)> = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT e.message_id, m.conversation_id, e.vec
                 FROM embeddings e JOIN messages m ON m.id = e.message_id
                 WHERE (?1 = '' OR m.sender = ?1)",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![role_filter]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let msg_id: String = row.get(0).map_err(|e| e.to_string())?;
            let conv_id: String = row.get(1).map_err(|e| e.to_string())?;
            let blob: Vec<u8> = row.get(2).map_err(|e| e.to_string())?;
            out.push((msg_id, conv_id, embed::blob_to_vec(&blob)));
        }
        out
        // MutexGuard dropped here
    };

    if candidates.is_empty() {
        return Ok(vec![]);
    }

    // 3. Score and rank — pure computation, no I/O
    let mut scored: Vec<(f32, String, String)> = candidates
        .into_iter()
        .map(|(msg_id, conv_id, vec)| (embed::cosine_sim(&query_vec, &vec), msg_id, conv_id))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // 4. Deduplicate by conversation and fetch metadata
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let conn = state.0.lock().map_err(|e| e.to_string())?;
    for (sim, msg_id, conv_id) in scored {
        if hits.len() >= limit { break; }
        if seen.contains(&conv_id) { continue; }

        #[allow(clippy::type_complexity)]
        let row: Option<(String, String, String, Option<String>, Option<String>, i64)> = conn
            .query_row(
                "SELECT m.text, c.title, c.platform, c.updated_at, c.created_at, c.message_count
                 FROM messages m JOIN conversations c ON c.id = m.conversation_id
                 WHERE m.id = ?1
                   AND (?2 = '' OR c.platform = ?2)
                   AND (?3 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) >= ?3)
                   AND (?4 = '' OR DATE(COALESCE(c.updated_at, c.created_at)) <= ?4)
                   AND (?5 = 0 OR c.message_count >= ?5)
                   AND (?6 = 0 OR c.message_count <= ?6)",
                params![msg_id, plat_filter, df, dt, min_msg, max_msg],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    ))
                },
            )
            .ok();

        let Some((text, title, platform, updated_at, created_at, _message_count)) = row else { continue };
        if !seen.insert(conv_id.clone()) { continue; }

        let snippet: String = text.chars().take(140).collect();
        let snippet = if text.chars().count() > 140 {
            format!("{snippet}…")
        } else {
            snippet
        };
        hits.push(SearchHit {
            ref_id: msg_id,
            conversation_id: conv_id,
            kind: format!("semantic:{:.2}", sim),
            snippet,
            conversation_title: title,
            platform,
            updated_at,
            created_at,
        });
    }

    Ok(hits)
}

// ─── Resume CC / Codex session in terminal ───────────────────────────────────

/// Open a terminal and run `command` (e.g. `claude --resume <id>`) inside it.
/// Reads `preferred_terminal` from settings; defaults to macOS Terminal.app.
/// Only macOS is supported; on other platforms this returns an error.
#[tauri::command]
pub fn launch_resume_terminal(
    state: State<DbState>,
    command: String,
    cwd: Option<String>,
) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Terminal resume is only supported on macOS".to_string());
    }
    if command.trim().is_empty() {
        return Err("Resume command is empty".to_string());
    }

    let preferred = {
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT value FROM settings WHERE key = 'preferred_terminal'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .unwrap_or_else(|| "terminal".to_string())
    };

    let full_cmd = match &cwd {
        Some(dir) if !dir.trim().is_empty() => {
            let escaped = shell_escape_single(dir);
            format!("cd {escaped} && {command}")
        }
        _ => command.clone(),
    };

    match preferred.as_str() {
        "iterm2" | "iterm" => launch_iterm(&full_cmd),
        _ => launch_macos_terminal(&full_cmd),
    }
}

fn shell_escape_single(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn escape_osascript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn launch_macos_terminal(command: &str) -> Result<(), String> {
    // In App Sandbox (MAS / sandbox test builds) the kernel blocks child
    // process spawning, so osascript can activate Terminal but "do script"
    // is denied and the command never runs.  Detect the sandbox by the
    // container env var and bail early so the JS clipboard fallback fires.
    if std::env::var("APP_SANDBOX_CONTAINER_ID").is_ok() {
        return Err("Terminal launch blocked in App Sandbox".to_string());
    }
    let escaped = escape_osascript(command);
    let script = format!(
        r#"tell application "Terminal"
    activate
    do script "{escaped}"
end tell"#
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .map_err(|e| format!("Failed to launch Terminal: {e}"))?;
    Ok(())
}

fn launch_iterm(command: &str) -> Result<(), String> {
    if std::env::var("APP_SANDBOX_CONTAINER_ID").is_ok() {
        return Err("Terminal launch blocked in App Sandbox".to_string());
    }
    let escaped = escape_osascript(command);
    let script = format!(
        r#"tell application "iTerm"
    activate
    create window with default profile
    tell current session of current window
        write text "{escaped}"
    end tell
end tell"#
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .map_err(|e| format!("Failed to launch iTerm: {e}"))?;
    Ok(())
}

// ─── Local agent-session scan / import ───────────────────────────────────────

// Read-only: walk the known agent directories and report how many sessions
// each holds. Runs off-thread since it touches the filesystem.
#[tauri::command]
pub async fn scan_agent_sources(
    overrides: Option<std::collections::HashMap<String, String>>,
) -> Result<Vec<AgentSource>, String> {
    let overrides = overrides.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || agent_scan::scan_agent_sources(&overrides))
        .await
        .map_err(|e| e.to_string())
}

// Parse the selected tools' sessions and persist them into the conversation
// store. Mirrors import_zip_file: blocking work off the main thread, progress
// over a dedicated Channel to preserve event ordering. `overrides` carries any
// directories the user picked manually in this dialog session (see
// scan_agent_sources) — not persisted, the frontend re-supplies them.
#[tauri::command]
pub async fn import_agent_sessions(
    app: tauri::AppHandle,
    tools: Vec<String>,
    overrides: Option<std::collections::HashMap<String, String>>,
    on_progress: Channel<ImportProgress>,
) -> Result<ImportSummary, String> {
    let batch_id = Uuid::new_v4().to_string();
    let overrides = overrides.unwrap_or_default();
    on_progress
        .send(ImportProgress { current: 0, total: 0, phase: "parse".to_string() })
        .ok();

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DbState>();
        let mut conn = state.0.lock().map_err(|e| e.to_string())?;
        agent_scan::import_agent_sessions(
            &mut conn,
            &tools,
            &overrides,
            &batch_id,
            Some(&move |current, total| {
                on_progress
                    .send(ImportProgress { current, total, phase: "db".to_string() })
                    .ok();
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod path_grouping_tests {
    use super::*;

    /// The point of list_conversations_by_path is grouping across tools —
    /// a project directory worked on with both Claude Code and Codex should
    /// show every session from either, sorted newest first, and nothing
    /// from an unrelated directory or an empty-summary (non-agent) platform.
    #[test]
    fn groups_claude_code_and_codex_sessions_under_the_same_path_and_excludes_others() {
        let db_path = std::env::temp_dir().join(format!("chatvault_test_path_group_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let conn = crate::db::open(&db_path).unwrap();
        conn.execute_batch(
            "
            INSERT INTO conversations (id, platform, title, summary, imported_at, updated_at)
                VALUES ('cc:1', 'claude-code', 'CC session', '/Users/dev/reg-factory', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
            INSERT INTO conversations (id, platform, title, summary, imported_at, updated_at)
                VALUES ('codex:1', 'codex', 'Codex session', '/Users/dev/reg-factory', '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z');
            INSERT INTO conversations (id, platform, title, summary, imported_at, updated_at)
                VALUES ('cc:2', 'claude-code', 'Other project', '/Users/dev/other-project', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
            INSERT INTO conversations (id, platform, title, imported_at)
                VALUES ('claude:1', 'claude', 'ZIP-imported, no cwd', '2026-01-01T00:00:00Z');
            ",
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT id, platform, title, summary, url, created_at, updated_at, message_count
                 FROM conversations WHERE summary = ?1 AND summary != ''
                 ORDER BY COALESCE(updated_at, created_at) DESC",
            )
            .unwrap();
        let ids: Vec<String> = stmt
            .query_map(params!["/Users/dev/reg-factory"], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            ids,
            vec!["codex:1", "cc:1"],
            "both tools' sessions for the path must come back, newest first, excluding the other project and the non-agent zip import"
        );

        let _ = std::fs::remove_file(&db_path);
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    /// Seeds one conversation with a message, a favorite on that message, a
    /// search_index row, and an embeddings row — one row in every table that
    /// references a conversation/message, whether via FK cascade or not.
    fn seed(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "
            INSERT INTO conversations (id, platform, title, imported_at, import_batch_id)
                VALUES ('conv-1', 'claude-code', 'Test', '2026-01-01T00:00:00Z', 'batch-1');
            INSERT INTO messages (id, conversation_id, sender, text, seq)
                VALUES ('msg-1', 'conv-1', 'human', 'hello', 0);
            INSERT INTO favorites (id, conversation_id, message_id, selected_text, created_at)
                VALUES ('fav-1', 'conv-1', 'msg-1', 'hello', '2026-01-01T00:00:00Z');
            INSERT INTO search_index (ref_id, conversation_id, kind, text)
                VALUES ('msg-1', 'conv-1', 'message', 'hello');
            INSERT INTO embeddings (message_id, model, vec) VALUES ('msg-1', 'test-model', x'00');
            ",
        )
        .unwrap();
    }

    fn counts(conn: &rusqlite::Connection) -> (i64, i64, i64, i64, i64) {
        let c = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        (
            c("SELECT COUNT(*) FROM conversations"),
            c("SELECT COUNT(*) FROM messages"),
            c("SELECT COUNT(*) FROM favorites"),
            c("SELECT COUNT(*) FROM search_index"),
            c("SELECT COUNT(*) FROM embeddings"),
        )
    }

    /// The risk this guards against: messages/favorites cascade via FK, but
    /// search_index (FTS5, no FK support) and embeddings (no FK defined) do
    /// not — a naive "just DELETE FROM conversations" would leave orphaned
    /// rows in both, silently bloating the FTS index and leaking stale
    /// embeddings for a message_id that no longer exists.
    #[test]
    fn delete_conversation_dependents_cleans_up_everything() {
        let db_path = std::env::temp_dir().join(format!("chatvault_test_delete_conv_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let mut conn = crate::db::open(&db_path).unwrap();
        seed(&conn);
        assert_eq!(counts(&conn), (1, 1, 1, 1, 1), "seed should populate every table once");

        let tx = conn.transaction().unwrap();
        delete_conversation_dependents(&tx, "conv-1").unwrap();
        tx.execute("DELETE FROM conversations WHERE id = 'conv-1'", []).unwrap();
        tx.commit().unwrap();

        assert_eq!(
            counts(&conn),
            (0, 0, 0, 0, 0),
            "conversation delete must clear messages/favorites (FK cascade) and search_index/embeddings (explicit)"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn delete_conversation_dependents_only_affects_the_target_conversation() {
        let db_path = std::env::temp_dir().join(format!("chatvault_test_delete_conv_scoped_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let mut conn = crate::db::open(&db_path).unwrap();
        seed(&conn);
        conn.execute_batch(
            "
            INSERT INTO conversations (id, platform, title, imported_at, import_batch_id)
                VALUES ('conv-2', 'claude-code', 'Other', '2026-01-01T00:00:00Z', 'batch-1');
            INSERT INTO messages (id, conversation_id, sender, text, seq)
                VALUES ('msg-2', 'conv-2', 'human', 'untouched', 0);
            INSERT INTO search_index (ref_id, conversation_id, kind, text)
                VALUES ('msg-2', 'conv-2', 'message', 'untouched');
            ",
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        delete_conversation_dependents(&tx, "conv-1").unwrap();
        tx.execute("DELETE FROM conversations WHERE id = 'conv-1'", []).unwrap();
        tx.commit().unwrap();

        let remaining_conv: i64 = conn.query_row("SELECT COUNT(*) FROM conversations WHERE id = 'conv-2'", [], |r| r.get(0)).unwrap();
        let remaining_msg: i64 = conn.query_row("SELECT COUNT(*) FROM messages WHERE conversation_id = 'conv-2'", [], |r| r.get(0)).unwrap();
        let remaining_fts: i64 = conn.query_row("SELECT COUNT(*) FROM search_index WHERE conversation_id = 'conv-2'", [], |r| r.get(0)).unwrap();
        assert_eq!((remaining_conv, remaining_msg, remaining_fts), (1, 1, 1), "unrelated conversation must survive untouched");

        let _ = std::fs::remove_file(&db_path);
    }
}
