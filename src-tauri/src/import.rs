use crate::models::{ImportSummary, NormalizedConversation, NormalizedMessage};
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;

fn read_conversations_json(zip_path: &str) -> Result<Value, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("无法打开文件: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("不是有效的zip文件: {e}"))?;

    // Collect matching entries: conversations.json or conversations-NNN.json (ChatGPT multi-file export).
    let mut candidates: Vec<(String, usize)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let basename = name.rsplit('/').next().unwrap_or(&name);
        if basename == "conversations.json"
            || (basename.starts_with("conversations-") && basename.ends_with(".json"))
        {
            candidates.push((name, i));
        }
    }

    if candidates.is_empty() {
        return Err("压缩包中未找到 conversations.json 或 conversations-NNN.json".to_string());
    }

    // Sort by name so multi-part files are merged in order.
    candidates.sort_by(|a, b| a.0.cmp(&b.0));

    if candidates.len() == 1 {
        let mut entry = archive.by_index(candidates[0].1).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .map_err(|e| format!("读取文件失败: {e}"))?;
        return serde_json::from_str(&buf).map_err(|e| format!("解析 JSON 失败: {e}"));
    }

    // Merge all arrays (ChatGPT splits export into conversations-000.json … conversations-NNN.json).
    let mut merged: Vec<Value> = Vec::new();
    for (_, idx) in candidates {
        let mut entry = archive.by_index(idx).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .map_err(|e| format!("读取文件失败: {e}"))?;
        let parsed: Value = serde_json::from_str(&buf).map_err(|e| format!("解析 JSON 失败: {e}"))?;
        if let Some(arr) = parsed.as_array() {
            merged.extend(arr.iter().cloned());
        }
    }
    Ok(Value::Array(merged))
}

fn detect_platform(arr: &[Value]) -> &'static str {
    if let Some(first) = arr.first() {
        if first.get("chat_messages").is_some() {
            return "claude";
        }
        if first.get("mapping").is_some() {
            // DeepSeek uses inserted_at (ISO string); ChatGPT uses create_time (float).
            if first.get("inserted_at").is_some() {
                return "deepseek";
            }
            return "chatgpt";
        }
    }
    "claude"
}

fn extract_claude_text(msg: &Value) -> String {
    if let Some(t) = msg.get("text").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            return t.to_string();
        }
    }
    if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
        let parts: Vec<String> = content
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    block.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();
        return parts.join("\n");
    }
    String::new()
}

fn parse_claude(arr: Vec<Value>) -> Vec<NormalizedConversation> {
    arr.into_iter()
        .filter_map(|conv| {
            let id = conv.get("uuid")?.as_str()?.to_string();
            let title = conv
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let summary = conv
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let created_at = conv.get("created_at").and_then(|v| v.as_str()).map(String::from);
            let updated_at = conv.get("updated_at").and_then(|v| v.as_str()).map(String::from);
            let messages: Vec<NormalizedMessage> = conv
                .get("chat_messages")
                .and_then(|v| v.as_array())
                .map(|msgs| {
                    msgs.iter()
                        .filter_map(|m| {
                            let mid = m.get("uuid")?.as_str()?.to_string();
                            let sender = m.get("sender").and_then(|v| v.as_str()).unwrap_or("human").to_string();
                            let text = extract_claude_text(m);
                            let mcreated = m.get("created_at").and_then(|v| v.as_str()).map(String::from);
                            Some(NormalizedMessage {
                                id: mid,
                                sender,
                                text,
                                created_at: mcreated,
                                kind: "text".to_string(),
                                model: None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(NormalizedConversation {
                cwd: String::new(),
                id: id.clone(),
                platform: "claude".to_string(),
                title,
                summary,
                url: format!("https://claude.ai/chat/{id}"),
                created_at,
                updated_at,
                messages,
            })
        })
        .collect()
}

// ChatGPT image filenames are "file-{id}-{suffix}.ext" where suffix is either a
// standard UUID (8-4-4-4-12 hex) or a compact 32-char hex. ChatGPT file IDs are
// base62 with no dashes, so the first '-' after "file-" is always the boundary.
fn strip_uuid_suffix(stem: &str) -> &str {
    if !stem.starts_with("file-") {
        return stem;
    }
    match stem[5..].find('-') {
        Some(pos) => &stem[..5 + pos],
        None => stem,
    }
}

fn image_mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp"         => Some("image/webp"),
        "gif"          => Some("image/gif"),
        "png"          => Some("image/png"),
        _              => None,
    }
}

// Build file_id → data-URI map from images stored in the ChatGPT export ZIP.
// ChatGPT stores user uploads under user_uploads/ and DALL-E images under
// dalle_generations/. We skip files larger than 10 MB to keep the DB sane.
//
// Newer exports store every attachment as extension-less "file-{id}.dat" and
// ship a separate conversation_asset_file_names.json mapping each .dat entry
// back to its original filename (which carries the real extension) — without
// that lookup every image is skipped since ".dat" isn't a recognized type.
fn build_chatgpt_image_map(zip_path: &str) -> std::collections::HashMap<String, String> {
    use base64::Engine;
    const MAX_BYTES: usize = 10_485_760; // 10 MB raw
    let mut map = std::collections::HashMap::new();
    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return map,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return map,
    };

    let mut real_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Ok(mut entry) = archive.by_name("conversation_asset_file_names.json") {
        let mut buf = String::new();
        if entry.read_to_string(&mut buf).is_ok() {
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&buf) {
                for (k, v) in obj {
                    if let Some(name) = v.as_str() {
                        real_names.insert(k, name.to_string());
                    }
                }
            }
        }
    }

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        let basename = name.rsplit('/').next().unwrap_or(&name).to_string();
        let dot = match basename.rfind('.') { Some(d) => d, None => continue };
        let (stem, ext) = (basename[..dot].to_string(), basename[dot + 1..].to_lowercase());

        let mime = if ext == "dat" {
            let real_ext = real_names
                .get(&basename)
                .and_then(|n| n.rsplit('.').next())
                .map(|e| e.to_lowercase());
            match real_ext.as_deref().and_then(image_mime_for_ext) {
                Some(m) => m,
                None => continue,
            }
        } else {
            match image_mime_for_ext(&ext) {
                Some(m) => m,
                None => continue,
            }
        };

        if entry.size() as usize > MAX_BYTES { continue; }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() { continue; }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        let file_id = strip_uuid_suffix(&stem);
        map.insert(file_id.to_string(), format!("data:{mime};base64,{b64}"));
    }
    map
}

// Strip markdown images whose src is a data URI so they don't bloat the FTS index.
fn strip_data_uris_for_fts(text: &str) -> std::borrow::Cow<str> {
    if !text.contains("](data:") {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(img_start) = rest.find("![") {
        out.push_str(&rest[..img_start]);
        let tail = &rest[img_start + 2..];
        // Expect ](data: somewhere after the alt text
        if let Some(bracket) = tail.find("](data:") {
            let after_paren = &tail[bracket + 2..];
            if let Some(close) = after_paren.find(')') {
                out.push_str("[图片]");
                rest = &after_paren[close + 1..];
                continue;
            }
        }
        out.push_str("![");
        rest = tail;
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn extract_chatgpt_text(
    message: &Value,
    img_map: &std::collections::HashMap<String, String>,
) -> String {
    let Some(parts) = message
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    else {
        return String::new();
    };

    let mut out: Vec<String> = Vec::new();
    for part in parts {
        if let Some(s) = part.as_str() {
            if !s.trim().is_empty() {
                out.push(s.to_string());
            }
        } else if let Some(obj) = part.as_object() {
            let ct = obj.get("content_type").and_then(|v| v.as_str()).unwrap_or("");
            if ct == "image_asset_pointer" {
                if let Some(ptr) = obj.get("asset_pointer").and_then(|v| v.as_str()) {
                    let file_id = ptr.strip_prefix("file-service://").unwrap_or(ptr);
                    if let Some(data_url) = img_map.get(file_id) {
                        out.push(format!("![图片]({data_url})"));
                    } else {
                        out.push("*[图片附件]*".to_string());
                    }
                }
            }
        }
    }
    out.join("\n\n")
}

fn unix_secs_to_iso(secs: f64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|dt| dt.to_rfc3339())
}

fn parse_chatgpt(
    arr: Vec<Value>,
    img_map: &std::collections::HashMap<String, String>,
) -> Vec<NormalizedConversation> {
    arr.into_iter()
        .filter_map(|conv| {
            let id = conv
                .get("conversation_id")
                .or_else(|| conv.get("id"))
                .and_then(|v| v.as_str())?
                .to_string();
            let title = conv.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let created_at = conv
                .get("create_time")
                .and_then(|v| v.as_f64())
                .and_then(unix_secs_to_iso);
            let updated_at = conv
                .get("update_time")
                .and_then(|v| v.as_f64())
                .and_then(unix_secs_to_iso);

            let mapping = conv.get("mapping").and_then(|v| v.as_object());
            let mut entries: Vec<(f64, NormalizedMessage)> = Vec::new();
            if let Some(map) = mapping {
                for (node_id, node) in map.iter() {
                    let Some(message) = node.get("message") else { continue };
                    if message.is_null() {
                        continue;
                    }
                    let role = message
                        .get("author")
                        .and_then(|a| a.get("role"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if role != "user" && role != "assistant" {
                        continue;
                    }
                    let text = extract_chatgpt_text(message, img_map);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let ts = message.get("create_time").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let sender = if role == "user" { "human" } else { "assistant" };
                    // Assistant turns record which model answered them.
                    let model = if sender == "assistant" {
                        message
                            .get("metadata")
                            .and_then(|m| m.get("model_slug"))
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    } else {
                        None
                    };
                    entries.push((
                        ts,
                        NormalizedMessage {
                            id: node_id.clone(),
                            sender: sender.to_string(),
                            text,
                            created_at: unix_secs_to_iso(ts),
                            kind: "text".to_string(),
                            model,
                        },
                    ));
                }
            }
            entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            let messages = entries.into_iter().map(|(_, m)| m).collect();

            Some(NormalizedConversation {
                cwd: String::new(),
                id: id.clone(),
                platform: "chatgpt".to_string(),
                title,
                summary: String::new(),
                url: format!("https://chatgpt.com/c/{id}"),
                created_at,
                updated_at,
                messages,
            })
        })
        .collect()
}

fn parse_deepseek(arr: Vec<Value>) -> Vec<NormalizedConversation> {
    arr.into_iter()
        .filter_map(|conv| {
            let id = conv.get("id")?.as_str()?.to_string();
            let title = conv.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let created_at = conv.get("inserted_at").and_then(|v| v.as_str()).map(String::from);
            let updated_at = conv.get("updated_at").and_then(|v| v.as_str()).map(String::from);

            let mapping = conv.get("mapping").and_then(|v| v.as_object())?;
            let mut messages: Vec<NormalizedMessage> = Vec::new();

            // Follow the main path from "root", always taking the last child at forks
            // (last child = most recent regeneration).
            let mut current = "root".to_string();
            loop {
                let node = match mapping.get(&current) {
                    Some(n) => n,
                    None => break,
                };

                if let Some(message) = node.get("message").filter(|m| !m.is_null()) {
                    if let Some(frags) = message.get("fragments").and_then(|v| v.as_array()) {
                        let is_request = frags.iter().any(|f| {
                            f.get("type").and_then(|v| v.as_str()) == Some("REQUEST")
                        });

                        let (sender, text) = if is_request {
                            let text = frags
                                .iter()
                                .filter_map(|f| {
                                    if f.get("type").and_then(|v| v.as_str()) == Some("REQUEST") {
                                        f.get("content").and_then(|v| v.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            ("human", text)
                        } else {
                            let mut parts: Vec<String> = Vec::new();

                            // Collect thinking fragments and format as markdown blockquote
                            let think: Vec<&str> = frags
                                .iter()
                                .filter_map(|f| {
                                    if f.get("type").and_then(|v| v.as_str()) == Some("THINK") {
                                        f.get("content").and_then(|v| v.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            if !think.is_empty() {
                                let quoted = think
                                    .join("\n")
                                    .lines()
                                    .map(|l| format!("> {l}"))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                parts.push(format!("**💭 思考过程**\n\n{quoted}"));
                            }

                            // Response text
                            let resp: Vec<&str> = frags
                                .iter()
                                .filter_map(|f| {
                                    if f.get("type").and_then(|v| v.as_str()) == Some("RESPONSE") {
                                        f.get("content").and_then(|v| v.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            if !resp.is_empty() {
                                parts.push(resp.join("\n"));
                            }

                            let text = if parts.len() > 1 {
                                parts.join("\n\n---\n\n")
                            } else {
                                parts.join("")
                            };
                            ("assistant", text)
                        };

                        if !text.trim().is_empty() {
                            let mid = node.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let ts = message.get("inserted_at").and_then(|v| v.as_str()).map(String::from);
                            // DeepSeek tags every node with the model, request
                            // turns included — only the answer was produced by it.
                            let model = if sender == "assistant" {
                                message.get("model").and_then(|v| v.as_str()).map(String::from)
                            } else {
                                None
                            };
                            messages.push(NormalizedMessage {
                                id: format!("{id}_{mid}"),
                                sender: sender.to_string(),
                                text,
                                created_at: ts,
                                kind: "text".to_string(),
                                model,
                            });
                        }
                    }
                }

                // Advance: take the last child (most recent regeneration at forks)
                match node.get("children").and_then(|v| v.as_array()) {
                    Some(ch) if !ch.is_empty() => {
                        current = ch.last().and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if current.is_empty() {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            Some(NormalizedConversation {
                cwd: String::new(),
                id: id.clone(),
                platform: "deepseek".to_string(),
                title,
                summary: String::new(),
                url: format!("https://chat.deepseek.com/"),
                created_at,
                updated_at,
                messages,
            })
        })
        .collect()
}

fn content_hash(conv: &NormalizedConversation) -> String {
    let mut hasher = Sha256::new();
    hasher.update(conv.title.as_bytes());
    for m in &conv.messages {
        hasher.update(m.id.as_bytes());
        hasher.update(m.text.as_bytes());
        // Part of the hash so sessions stored before models were recorded are
        // seen as changed on the next scan and get backfilled, instead of
        // being skipped as unchanged forever.
        hasher.update(m.model.as_deref().unwrap_or("").as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Distinct models used across a conversation, in first-appearance order.
/// Stored comma-joined on `conversations.models` so the list view and the
/// model filter don't have to scan the messages table.
fn distinct_models(conv: &NormalizedConversation) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for m in &conv.messages {
        if let Some(model) = m.model.as_deref() {
            if !model.is_empty() && !out.contains(&model) {
                out.push(model);
            }
        }
    }
    out
}

/// Persist a batch of normalized conversations into an open transaction,
/// returning (added, updated, skipped). Shared by the ZIP importer and the
/// local-agent-session importer so both apply identical dedup / FTS logic.
/// Progress reporter: `(phase, current, total)`. A `total` of 0 means the phase
/// has no countable unit of work — the UI shows an indeterminate state for it.
pub(crate) type ProgressFn<'a> = &'a dyn Fn(&str, usize, usize);

pub(crate) fn persist_conversations(
    tx: &rusqlite::Transaction,
    conversations: &[NormalizedConversation],
    batch_id: &str,
    now: &str,
    on_progress: Option<ProgressFn>,
) -> Result<(i64, i64, i64), String> {
    let mut added = 0i64;
    let mut updated = 0i64;
    let mut skipped = 0i64;

    // Pass 1 — classify. Hashing and the existence lookup are cheap; doing them
    // up front means the rows that need clearing are known as one set, which is
    // what makes the bulk delete below possible.
    let mut pending: Vec<(&NormalizedConversation, String)> = Vec::new();
    let mut stale_ids: Vec<&str> = Vec::new();
    {
        let mut existing_hash_stmt = tx
            .prepare("SELECT content_hash FROM conversations WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        for (idx, conv) in conversations.iter().enumerate() {
            if let Some(cb) = on_progress {
                if idx % 25 == 0 || idx + 1 == conversations.len() {
                    cb("compare", idx + 1, conversations.len());
                }
            }
            let hash = content_hash(conv);
            let existing: Option<String> = existing_hash_stmt
                .query_row(params![conv.id], |row| row.get(0))
                .ok();
            match existing {
                Some(old_hash) if old_hash == hash => {
                    skipped += 1;
                    continue;
                }
                Some(_) => {
                    stale_ids.push(&conv.id);
                    updated += 1;
                }
                None => added += 1,
            }
            pending.push((conv, hash));
        }
    }

    // Pass 2 — clear the old rows of every changed conversation in one go.
    //
    // This has to be a single statement rather than one per conversation:
    // `search_index` is an FTS5 table whose `conversation_id` is UNINDEXED, so
    // every `DELETE ... WHERE conversation_id = ?` scans the whole index. Once
    // per conversation that is quadratic — re-importing a 1.7k-conversation
    // export spent almost all of its time there. One scan covers them all.
    if !stale_ids.is_empty() {
        // No countable unit here — it is deliberately one statement — so the
        // phase is announced and the UI shows an indeterminate state for it
        // rather than a bar that sits still.
        if let Some(cb) = on_progress {
            cb("clean", 0, 0);
        }
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS stale_conversations (id TEXT PRIMARY KEY);
             DELETE FROM stale_conversations;",
        )
        .map_err(|e| e.to_string())?;
        {
            let mut ins = tx
                .prepare("INSERT OR IGNORE INTO stale_conversations (id) VALUES (?1)")
                .map_err(|e| e.to_string())?;
            for id in &stale_ids {
                ins.execute(params![id]).map_err(|e| e.to_string())?;
            }
        }
        tx.execute(
            "DELETE FROM messages WHERE conversation_id IN (SELECT id FROM stale_conversations)",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM search_index WHERE conversation_id IN (SELECT id FROM stale_conversations)",
            [],
        )
        .map_err(|e| e.to_string())?;
        tx.execute_batch("DELETE FROM stale_conversations;")
            .map_err(|e| e.to_string())?;
    }

    // Pass 3 — write. Statements are prepared once and reused; `tx.execute(sql, ..)`
    // re-parses the SQL on every call, which is measurable when it runs once per
    // message across tens of thousands of them.
    let mut conv_stmt = tx
        .prepare(
            "INSERT INTO conversations (id, platform, title, summary, url, created_at, updated_at, message_count, models, content_hash, imported_at, import_batch_id, cwd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                platform=excluded.platform, title=excluded.title, summary=excluded.summary,
                url=excluded.url, created_at=excluded.created_at, updated_at=excluded.updated_at,
                message_count=excluded.message_count, models=excluded.models,
                content_hash=excluded.content_hash, cwd=excluded.cwd,
                imported_at=excluded.imported_at, import_batch_id=excluded.import_batch_id",
        )
        .map_err(|e| e.to_string())?;
    let mut msg_stmt = tx
        .prepare(
            "INSERT INTO messages (id, conversation_id, sender, text, created_at, seq, kind, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET text=excluded.text, sender=excluded.sender, created_at=excluded.created_at, seq=excluded.seq, kind=excluded.kind, model=excluded.model",
        )
        .map_err(|e| e.to_string())?;
    let mut fts_stmt = tx
        .prepare("INSERT INTO search_index (ref_id, conversation_id, kind, text) VALUES (?1, ?2, ?3, ?4)")
        .map_err(|e| e.to_string())?;

    let total = pending.len();
    for (idx, (conv, hash)) in pending.iter().enumerate() {
        if let Some(cb) = on_progress {
            if idx % 5 == 0 || idx + 1 == total {
                cb("db", idx + 1, total);
            }
        }

        conv_stmt
            .execute(params![
                conv.id,
                conv.platform,
                conv.title,
                conv.summary,
                conv.url,
                conv.created_at,
                conv.updated_at,
                conv.messages.len() as i64,
                distinct_models(conv).join(","),
                hash,
                now,
                batch_id,
                conv.cwd
            ])
            .map_err(|e| e.to_string())?;

        fts_stmt
            .execute(params![
                conv.id,
                conv.id,
                "title",
                format!("{} {}", conv.title, conv.summary)
            ])
            .map_err(|e| e.to_string())?;

        for (seq, msg) in conv.messages.iter().enumerate() {
            msg_stmt
                .execute(params![
                    msg.id,
                    conv.id,
                    msg.sender,
                    msg.text,
                    msg.created_at,
                    seq as i64,
                    msg.kind,
                    msg.model
                ])
                .map_err(|e| e.to_string())?;

            let fts_text = strip_data_uris_for_fts(&msg.text);
            fts_stmt
                .execute(params![msg.id, conv.id, "message", fts_text.as_ref()])
                .map_err(|e| e.to_string())?;
        }
    }

    Ok((added, updated, skipped))
}

pub fn import_zip(
    conn: &mut Connection,
    zip_path: &str,
    batch_id: &str,
    on_progress: Option<ProgressFn>,
) -> Result<ImportSummary, String> {
    let raw = read_conversations_json(zip_path)?;
    let arr = raw.as_array().ok_or_else(|| "conversations.json 格式不正确".to_string())?.clone();
    let platform = detect_platform(&arr);
    let total = arr.len() as i64;

    let mut conversations = match platform {
        "claude"   => parse_claude(arr),
        "deepseek" => parse_deepseek(arr),
        _ => {
            let img_map = build_chatgpt_image_map(zip_path);
            parse_chatgpt(arr, &img_map)
        }
    };

    // Derive timestamps from messages so we always display actual conversation time,
    // not export/metadata time (DeepSeek's conversation-level updated_at is often
    // the export timestamp rather than the last message time).
    for conv in conversations.iter_mut() {
        let first_ts = conv.messages.first().and_then(|m| m.created_at.clone());
        let last_ts  = conv.messages.last().and_then(|m| m.created_at.clone());
        if conv.created_at.is_none() {
            conv.created_at = first_ts;
        }
        if last_ts.is_some() {
            conv.updated_at = last_ts; // prefer actual last-message time
        }
    }

    let now = chrono::Utc::now().to_rfc3339();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (added, updated, skipped) =
        persist_conversations(&tx, &conversations, batch_id, &now, on_progress)?;

    tx.execute(
        "INSERT INTO import_batches (id, source_file, platform, imported_at, added_count, updated_count, skipped_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![batch_id, zip_path, platform, now, added, updated, skipped],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;

    Ok(ImportSummary {
        platform: platform.to_string(),
        added,
        updated,
        skipped,
        total_in_file: total,
    })
}

#[cfg(test)]
mod parser_version_tests {
    use super::*;

    fn conv(text: &str) -> NormalizedConversation {
        NormalizedConversation {
            cwd: String::new(),
            id: "conv-1".to_string(),
            platform: "claude-code".to_string(),
            title: "Session".to_string(),
            summary: String::new(),
            url: String::new(),
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            updated_at: Some("2026-01-01T00:00:00Z".to_string()),
            messages: vec![NormalizedMessage {
                id: "msg-1".to_string(),
                sender: "assistant".to_string(),
                text: text.to_string(),
                created_at: Some("2026-01-01T00:00:00Z".to_string()),
                kind: "text".to_string(),
                model: None,
            }],
        }
    }

    /// Reproduces the real 4b180c9 scenario: a Claude Code session containing
    /// an image block. The "old parser" drops the block entirely (matching
    /// `extract_plain_text`'s pre-fix behavior); the "new parser" emits
    /// markdown image syntax for the exact same source file. Re-persisting
    /// with the new parser's output — same conversation id, same message id —
    /// must overwrite the stored text, not skip it as a duplicate.
    #[test]
    fn reimport_with_changed_parser_output_updates_stored_text_and_search_index() {
        let db_path = std::env::temp_dir().join("chatvault_test_parser_version_a.db");
        let _ = std::fs::remove_file(&db_path);
        let mut db_conn = crate::db::open(&db_path).unwrap();

        let old_parser_text = "Here is the screenshot."; // image block silently dropped
        let new_parser_text = "Here is the screenshot.\n\n![](<data:image/png;base64,AAAA>)";

        // First import: old parser.
        let tx = db_conn.transaction().unwrap();
        let (added1, updated1, skipped1) =
            persist_conversations(&tx, &[conv(old_parser_text)], "batch-1", "2026-01-01T00:00:00Z", None)
                .unwrap();
        tx.commit().unwrap();
        assert_eq!((added1, updated1, skipped1), (1, 0, 0));

        let stored: String = db_conn
            .query_row("SELECT text FROM messages WHERE id = 'msg-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, old_parser_text, "first import should store the old parser's output verbatim");

        // Second import: same conversation id, same file on disk, but the
        // parser was upgraded to extract images — simulates the user
        // re-importing the same zip after a code update.
        let tx = db_conn.transaction().unwrap();
        let (added2, updated2, skipped2) =
            persist_conversations(&tx, &[conv(new_parser_text)], "batch-2", "2026-01-02T00:00:00Z", None)
                .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            (added2, updated2, skipped2),
            (0, 1, 0),
            "changed parser output must be treated as an update, not a duplicate skip"
        );

        let stored_after: String = db_conn
            .query_row("SELECT text FROM messages WHERE id = 'msg-1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            stored_after, new_parser_text,
            "reimport must overwrite the message text with the new parser's output, image markdown included"
        );

        // The FTS index is rebuilt too, so search reflects the refreshed content.
        let fts_hit: i64 = db_conn
            .query_row(
                "SELECT COUNT(*) FROM search_index WHERE ref_id = 'msg-1' AND text MATCH 'screenshot'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_hit, 1, "search index should be refreshed to match the updated text");

        let _ = std::fs::remove_file(&db_path);
    }

    /// Sanity check: re-persisting identical output (parser unchanged) is a
    /// no-op skip, so an ordinary reimport doesn't churn the database.
    #[test]
    fn reimport_with_unchanged_parser_output_is_skipped() {
        let db_path = std::env::temp_dir().join("chatvault_test_parser_version_b.db");
        let _ = std::fs::remove_file(&db_path);
        let mut db_conn = crate::db::open(&db_path).unwrap();

        let text = "Nothing changed here.";
        let tx = db_conn.transaction().unwrap();
        persist_conversations(&tx, &[conv(text)], "batch-1", "2026-01-01T00:00:00Z", None).unwrap();
        tx.commit().unwrap();

        let tx = db_conn.transaction().unwrap();
        let (added, updated, skipped) =
            persist_conversations(&tx, &[conv(text)], "batch-2", "2026-01-02T00:00:00Z", None).unwrap();
        tx.commit().unwrap();
        assert_eq!((added, updated, skipped), (0, 0, 1));

        let _ = std::fs::remove_file(&db_path);
    }
}

#[cfg(test)]
mod model_persistence_tests {
    use super::*;

    fn msg(id: &str, sender: &str, model: Option<&str>) -> NormalizedMessage {
        NormalizedMessage {
            id: id.to_string(),
            sender: sender.to_string(),
            text: format!("text of {id}"),
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            kind: "text".to_string(),
            model: model.map(String::from),
        }
    }

    /// The per-message model lands on `messages.model`, and the conversation
    /// row carries the distinct ones (first-appearance order, no repeats) so
    /// the list view and model filter never touch the messages table.
    #[test]
    fn persist_stores_per_message_models_and_the_conversation_model_list() {
        let db_path = std::env::temp_dir().join(format!("chatvault_test_models_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db_path);
        let mut db_conn = crate::db::open(&db_path).unwrap();

        let conv = NormalizedConversation {
            cwd: String::new(),
            id: "cc:models".to_string(),
            platform: "claude-code".to_string(),
            title: "Mixed session".to_string(),
            summary: String::new(),
            url: String::new(),
            created_at: Some("2026-01-01T00:00:00Z".to_string()),
            updated_at: Some("2026-01-01T00:00:00Z".to_string()),
            messages: vec![
                msg("m1", "human", None),
                msg("m2", "assistant", Some("claude-sonnet-4-6")),
                msg("m3", "assistant", Some("claude-opus-5")),
                msg("m4", "assistant", Some("claude-sonnet-4-6")),
            ],
        };

        let tx = db_conn.transaction().unwrap();
        persist_conversations(&tx, &[conv], "batch-1", "2026-01-01T00:00:00Z", None).unwrap();
        tx.commit().unwrap();

        let models: String = db_conn
            .query_row("SELECT models FROM conversations WHERE id = 'cc:models'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(models, "claude-sonnet-4-6,claude-opus-5");

        let per_message: Vec<Option<String>> = db_conn
            .prepare("SELECT model FROM messages WHERE conversation_id = 'cc:models' ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            per_message,
            vec![
                None,
                Some("claude-sonnet-4-6".to_string()),
                Some("claude-opus-5".to_string()),
                Some("claude-sonnet-4-6".to_string()),
            ]
        );

        let _ = std::fs::remove_file(&db_path);
    }
}
