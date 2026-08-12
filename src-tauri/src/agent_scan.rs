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
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// A discovered agent tool with a count of the sessions found on disk. Always
/// one entry per known tool (see `tool_defs`) — unlike a "found or omitted"
/// list, this lets the UI offer a manual folder picker for a tool even when
/// its default directory couldn't be read (missing, moved, or — on a sandboxed
/// macOS build — blocked by the App Sandbox with no persisted grant).
#[derive(Debug, Serialize, Clone)]
pub struct AgentSource {
    pub tool: String,          // stable id, e.g. "claude-code" | "codex"
    pub label: String,         // human label, e.g. "Claude Code"
    pub dir: String,           // absolute directory that was scanned (empty if unresolvable)
    pub session_count: usize,  // number of parseable session files
    pub message_count: usize,  // total user/assistant turns across sessions
    pub accessible: bool,      // false if the directory couldn't be read at all
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

/// `overrides` lets the caller (frontend, via a manual folder picker) point a
/// tool at a directory other than its default — used when the default path
/// doesn't exist, moved, or isn't readable (e.g. a sandboxed macOS build
/// without a persisted grant for `~/.claude`). The override is session-only:
/// nothing is written to disk here, the caller re-supplies it each call.
fn resolve_dir(def: &ToolDef, overrides: &HashMap<String, String>) -> Option<PathBuf> {
    if let Some(p) = overrides.get(def.tool) {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    (def.dir)()
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

/// Scan every known tool's directory (default path, or the caller-supplied
/// override) and report what each holds. Always returns one entry per known
/// tool — including ones that are empty, missing, or unreadable — so the UI
/// can offer a manual folder picker for exactly the tools that need it.
pub fn scan_agent_sources(overrides: &HashMap<String, String>) -> Vec<AgentSource> {
    let mut sources = Vec::new();
    for def in tool_defs() {
        let dir = resolve_dir(&def, overrides);
        let accessible = dir.as_deref().map(is_readable_dir).unwrap_or(false);

        let mut session_count = 0usize;
        let mut message_count = 0usize;
        if accessible {
            let mut files = Vec::new();
            collect_jsonl_files(dir.as_deref().unwrap(), &mut files);
            for path in &files {
                if let Some(conv) = (def.parse)(path) {
                    if conv.messages.is_empty() {
                        continue;
                    }
                    session_count += 1;
                    message_count += conv.messages.len();
                }
            }
        }

        sources.push(AgentSource {
            tool: def.tool.to_string(),
            label: def.label.to_string(),
            dir: dir.map(|d| d.to_string_lossy().to_string()).unwrap_or_default(),
            session_count,
            message_count,
            accessible,
        });
    }
    sources
}

fn is_readable_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok()
}

/// Parse + persist sessions for the requested tools. `tools` is a list of
/// `AgentSource.tool` ids; an empty list imports every supported tool.
/// `overrides` mirrors `scan_agent_sources` — same session-only directory
/// overrides picked via the manual folder picker.
pub fn import_agent_sessions(
    conn: &mut Connection,
    tools: &[String],
    overrides: &HashMap<String, String>,
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
        let Some(dir) = resolve_dir(&def, overrides) else { continue };
        if !is_readable_dir(&dir) {
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
                        "image" => {
                            let source = item.get("source")?;
                            let src_type = source.get("type").and_then(Value::as_str)?;
                            if src_type == "base64" {
                                let media_type = source
                                    .get("media_type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("image/png");
                                let data = source.get("data").and_then(Value::as_str)?;
                                Some(format!("![](<data:{media_type};base64,{data}>)"))
                            } else {
                                None
                            }
                        }
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

/// Claude Code and Codex both write CLI-injected content (local slash-command
/// output, hook results, `<environment_context>` wrappers, …) into the
/// session log as ordinary `role: "user"` turns — that's just the mechanism
/// they use to feed local context back to the model, not something the human
/// actually typed. Both wrap it in bare `<tag>...</tag>` markup, which real
/// prose essentially never starts a message with, so that's the signal used
/// to tell it apart from an actual human turn.
fn is_synthetic_local_wrapper(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('<') && t.contains("</")
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
        // "meta" = CLI-injected local content riding on a user-role turn —
        // shown in the transcript but kept out of the human-input count/nav
        // (ConversationDetail's right panel, title generation) and search
        // role scoping, since it isn't something the person actually typed.
        let sender = if ty != "user" {
            "assistant"
        } else if is_synthetic_local_wrapper(&text) {
            "meta"
        } else {
            "human"
        };
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

#[cfg(test)]
mod meta_sender_tests {
    use super::*;
    use std::io::Write;

    /// Reproduces the real-world case reported against a `reg-factory` Claude
    /// Code session: CLI-injected local-command output/caveats/`/model`
    /// echoes ride on `role: "user"` turns in the JSONL. They must be tagged
    /// "meta" — not "human" — so they don't pollute ConversationDetail's
    /// right-panel "user inputs" nav or get picked as the conversation title.
    #[test]
    fn claude_code_synthetic_user_turns_are_tagged_meta_not_human() {
        let dir = std::env::temp_dir().join(format!("cc_meta_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","cwd":"/tmp/reg-factory","timestamp":"2026-01-01T00:00:00Z","uuid":"u1","message":{{"content":"python check_port_countries.py --host 192.168.77.11"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","uuid":"a1","message":{{"content":"Let me check that."}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-01-01T00:00:02Z","uuid":"u2","message":{{"content":"<local-command-stdout>Set model to Sonnet 5</local-command-stdout>"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-01-01T00:00:03Z","uuid":"u3","message":{{"content":"<command-name>/model</command-name> <command-message>model</command-message> <command-args></command-args>"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-01-01T00:00:04Z","uuid":"u4","message":{{"content":"这个probe_failed什么意思？"}}}}"#
        )
        .unwrap();
        drop(f);

        let conv = parse_claude_code_session(&path).expect("should parse");
        let senders: Vec<&str> = conv.messages.iter().map(|m| m.sender.as_str()).collect();
        assert_eq!(
            senders,
            vec!["human", "assistant", "meta", "meta", "human"],
            "CLI-injected wrapper turns must be tagged meta, real prose stays human"
        );

        // Title generation must skip the meta turns and pick the real first
        // human message, not the local-command-stdout content.
        assert!(
            conv.title.contains("python check_port_countries.py") || conv.title.contains("reg-factory"),
            "title should be built from the real human turn, got: {}",
            conv.title
        );
        assert!(
            !conv.title.contains("local-command-stdout") && !conv.title.contains("Set model"),
            "title must not be built from synthetic CLI content, got: {}",
            conv.title
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
