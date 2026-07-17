// Proactively discover conversation histories written by local coding agents
// (Claude Code, Codex CLI) and import them into the same conversation store the
// ZIP importer feeds. Reading is on-demand: nothing is imported until the user
// confirms in the UI. We only pull natural-language user/assistant turns —
// tool calls, thinking blocks, and instruction wrappers are skipped so the
// stored transcript stays readable and the FTS index stays clean.

use crate::import::persist_conversations;
use crate::models::{ImportSummary, NormalizedConversation, NormalizedMessage};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// A discovered agent tool with a count of the sessions found on disk.
#[derive(Debug, Serialize, Clone)]
pub struct AgentSource {
    pub tool: String,          // stable id, e.g. "claude-code" | "codex"
    pub label: String,         // human label, e.g. "Claude Code"
    pub dir: String,           // absolute directory that was scanned
    pub session_count: usize,  // number of parseable session files
    pub message_count: usize,  // total user/assistant turns across sessions
}

struct ToolDef {
    tool: &'static str,
    label: &'static str,
    dir: fn() -> Option<PathBuf>,
    parse: fn(&Path) -> Option<NormalizedConversation>,
}

fn claude_code_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

fn codex_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("sessions"))
}

fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            tool: "claude-code",
            label: "Claude Code",
            dir: claude_code_dir,
            parse: parse_claude_code_session,
        },
        ToolDef {
            tool: "codex",
            label: "Codex",
            dir: codex_dir,
            parse: parse_codex_session,
        },
    ]
}

/// Recursively collect every `.jsonl` file beneath `root`.
fn collect_jsonl_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

/// Scan all supported tool directories and report which ones hold sessions.
/// Tools whose directory is missing or empty are omitted from the result.
pub fn scan_agent_sources() -> Vec<AgentSource> {
    let mut sources = Vec::new();
    for def in tool_defs() {
        let Some(dir) = (def.dir)() else { continue };
        if !dir.exists() {
            continue;
        }
        let mut files = Vec::new();
        collect_jsonl_files(&dir, &mut files);

        let mut session_count = 0usize;
        let mut message_count = 0usize;
        for path in &files {
            if let Some(conv) = (def.parse)(path) {
                if conv.messages.is_empty() {
                    continue;
                }
                session_count += 1;
                message_count += conv.messages.len();
            }
        }

        if session_count > 0 {
            sources.push(AgentSource {
                tool: def.tool.to_string(),
                label: def.label.to_string(),
                dir: dir.to_string_lossy().to_string(),
                session_count,
                message_count,
            });
        }
    }
    sources
}

/// Parse + persist sessions for the requested tools. `tools` is a list of
/// `AgentSource.tool` ids; an empty list imports every supported tool.
pub fn import_agent_sessions(
    conn: &mut Connection,
    tools: &[String],
    batch_id: &str,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> Result<ImportSummary, String> {
    let want = |t: &str| tools.is_empty() || tools.iter().any(|x| x == t);

    let mut conversations: Vec<NormalizedConversation> = Vec::new();
    let mut tools_used: Vec<&str> = Vec::new();

    for def in tool_defs() {
        if !want(def.tool) {
            continue;
        }
        let Some(dir) = (def.dir)() else { continue };
        if !dir.exists() {
            continue;
        }
        let mut files = Vec::new();
        collect_jsonl_files(&dir, &mut files);
        let before = conversations.len();
        for path in &files {
            if let Some(conv) = (def.parse)(path) {
                if !conv.messages.is_empty() {
                    conversations.push(conv);
                }
            }
        }
        if conversations.len() > before {
            tools_used.push(def.tool);
        }
    }

    let total = conversations.len() as i64;
    let now = chrono::Utc::now().to_rfc3339();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (added, updated, skipped) =
        persist_conversations(&tx, &conversations, batch_id, &now, on_progress)?;

    let platform = if tools_used.len() == 1 {
        tools_used[0].to_string()
    } else {
        "agent".to_string()
    };
    tx.execute(
        "INSERT INTO import_batches (id, source_file, platform, imported_at, added_count, updated_count, skipped_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![batch_id, "local-agent-scan", platform, now, added, updated, skipped],
    )
    .map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;

    Ok(ImportSummary {
        platform,
        added,
        updated,
        skipped,
        total_in_file: total,
    })
}

// ─── Shared text extraction ──────────────────────────────────────────────────

/// Pull plain text out of a message `content` field that may be a string or an
/// array of typed blocks. Only natural-language blocks (text / input_text /
/// output_text) are kept; tool_use, tool_result, thinking, images, etc. are
/// dropped so the transcript reads like a conversation.
fn extract_plain_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
                    match ty {
                        "text" | "input_text" | "output_text" | "" => item
                            .get("text")
                            .or_else(|| item.get("input_text"))
                            .or_else(|| item.get("output_text"))
                            .and_then(Value::as_str)
                            .map(|s| s.to_string()),
                        _ => None,
                    }
                })
                .filter(|s| !s.trim().is_empty())
                .collect();
            parts.join("\n\n")
        }
        _ => String::new(),
    }
}

fn basename(path: &str) -> Option<String> {
    let normalized = path.trim().trim_end_matches(['/', '\\']);
    normalized
        .split(['/', '\\'])
        .next_back()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn build_title(cwd: Option<&str>, first_user: Option<&str>) -> String {
    let project = cwd.and_then(basename);
    let snippet = first_user.map(|s| {
        let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
        let trimmed: String = one_line.chars().take(60).collect();
        if one_line.chars().count() > 60 {
            format!("{trimmed}…")
        } else {
            trimmed
        }
    });
    match (project, snippet) {
        (Some(p), Some(s)) if !s.is_empty() => format!("{p} · {s}"),
        (Some(p), _) => p,
        (None, Some(s)) if !s.is_empty() => s,
        _ => "未命名会话".to_string(),
    }
}

// ─── Claude Code (~/.claude/projects/**/<session>.jsonl) ─────────────────────

fn parse_claude_code_session(path: &Path) -> Option<NormalizedConversation> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let session_id = path.file_stem()?.to_str()?.to_string();
    let mut cwd: Option<String> = None;
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut created_at: Option<String> = None;
    let mut updated_at: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
        if ty != "user" && ty != "assistant" {
            continue;
        }
        if cwd.is_none() {
            cwd = value.get("cwd").and_then(Value::as_str).map(String::from);
        }
        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };
        // Skip user turns that only carry a tool_result payload.
        if ty == "user" {
            if let Some(Value::Array(items)) = message.get("content") {
                let all_tool = !items.is_empty()
                    && items.iter().all(|it| {
                        it.get("type").and_then(Value::as_str) == Some("tool_result")
                    });
                if all_tool {
                    continue;
                }
            }
        }
        let text = message
            .get("content")
            .map(extract_plain_text)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let ts = value.get("timestamp").and_then(Value::as_str).map(String::from);
        if created_at.is_none() {
            created_at = ts.clone();
        }
        if ts.is_some() {
            updated_at = ts.clone();
        }
        let sender = if ty == "user" { "human" } else { "assistant" };
        let msg_id = value
            .get("uuid")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| format!("{session_id}_{}", messages.len()));
        messages.push(NormalizedMessage {
            id: msg_id,
            sender: sender.to_string(),
            text,
            created_at: ts,
        });
    }

    if messages.is_empty() {
        return None;
    }

    let first_user = messages
        .iter()
        .find(|m| m.sender == "human")
        .map(|m| m.text.as_str());
    let title = build_title(cwd.as_deref(), first_user);

    Some(NormalizedConversation {
        id: format!("cc:{session_id}"),
        platform: "claude-code".to_string(),
        title,
        summary: cwd.unwrap_or_default(),
        url: String::new(),
        created_at,
        updated_at,
        messages,
    })
}

// ─── Codex CLI (~/.codex/sessions/**/rollout-*.jsonl) ────────────────────────

fn parse_codex_session(path: &Path) -> Option<NormalizedConversation> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut messages: Vec<NormalizedMessage> = Vec::new();
    let mut created_at: Option<String> = None;
    let mut updated_at: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
        let ts = value.get("timestamp").and_then(Value::as_str).map(String::from);

        if ty == "session_meta" {
            let payload = value.get("payload");
            session_id = payload
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str)
                .map(String::from);
            cwd = payload
                .and_then(|p| p.get("cwd"))
                .and_then(Value::as_str)
                .map(String::from);
            if created_at.is_none() {
                created_at = ts;
            }
            continue;
        }

        if ty != "response_item" {
            continue;
        }
        let payload = match value.get("payload") {
            Some(p) => p,
            None => continue,
        };
        if payload.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
        // Only real conversation turns — skip developer/system/instruction roles.
        if role != "user" && role != "assistant" {
            continue;
        }
        let text = payload
            .get("content")
            .map(extract_plain_text)
            .unwrap_or_default();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        // The first user turn is Codex's <environment_context>/<user_instructions>
        // wrapper, not a human message — drop obvious wrappers.
        if role == "user" && trimmed.starts_with('<') && trimmed.contains("</") {
            continue;
        }
        if created_at.is_none() {
            created_at = ts.clone();
        }
        if ts.is_some() {
            updated_at = ts.clone();
        }
        let sender = if role == "user" { "human" } else { "assistant" };
        let sid = session_id.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("codex")
                .to_string()
        });
        messages.push(NormalizedMessage {
            id: format!("{sid}_{}", messages.len()),
            sender: sender.to_string(),
            text,
            created_at: ts,
        });
    }

    if messages.is_empty() {
        return None;
    }

    let session_id = session_id.or_else(|| {
        path.file_stem().and_then(|s| s.to_str()).map(String::from)
    })?;

    let first_user = messages
        .iter()
        .find(|m| m.sender == "human")
        .map(|m| m.text.as_str());
    let title = build_title(cwd.as_deref(), first_user);

    Some(NormalizedConversation {
        id: format!("codex:{session_id}"),
        platform: "codex".to_string(),
        title,
        summary: cwd.unwrap_or_default(),
        url: String::new(),
        created_at,
        updated_at,
        messages,
    })
}
