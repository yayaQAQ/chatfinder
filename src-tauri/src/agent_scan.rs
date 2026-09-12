// Proactively discover conversation histories written by local coding agents
// (Claude Code, Codex CLI) and import them into the same conversation store the
// ZIP importer feeds. Reading is on-demand: nothing is imported until the user
// confirms in the UI. We only pull natural-language user/assistant turns —
// tool calls, thinking blocks, and instruction wrappers are skipped so the
// stored transcript stays readable and the FTS index stays clean.

use crate::import::{persist_conversations, ProgressFn};
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
    on_progress: Option<ProgressFn>,
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

// ─── Shared content extraction ───────────────────────────────────────────────

/// Pull every content block out of a message `content` field (string or array
/// of typed blocks) as `(kind, text)` pairs. Unlike the old text-only
/// extraction, tool_use / tool_result / thinking are kept — tagged with their
/// kind so the frontend can filter which parts to show. tool_use and
/// tool_result are formatted as markdown code blocks; text and thinking are
/// returned verbatim.
fn extract_blocks(content: &Value) -> Vec<(String, String)> {
    match content {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                vec![]
            } else {
                vec![("text".to_string(), t.to_string())]
            }
        }
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
                match ty {
                    "text" | "input_text" | "output_text" | "" => {
                        let s = item
                            .get("text")
                            .or_else(|| item.get("input_text"))
                            .or_else(|| item.get("output_text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !s.trim().is_empty() {
                            out.push(("text".to_string(), s.to_string()));
                        }
                    }
                    "tool_use" => out.push(("tool_use".to_string(), format_tool_use(item))),
                    "tool_result" => out.push(("tool_result".to_string(), format_tool_result(item))),
                    "thinking" => {
                        let s = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !s.trim().is_empty() {
                            out.push(("thinking".to_string(), s.to_string()));
                        }
                    }
                    "image" => {
                        if let Some(source) = item.get("source") {
                            let src_type = source.get("type").and_then(Value::as_str).unwrap_or("");
                            if src_type == "base64" {
                                let media_type = source
                                    .get("media_type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("image/png");
                                let data = source.get("data").and_then(Value::as_str).unwrap_or("");
                                if !data.is_empty() {
                                    out.push((
                                        "text".to_string(),
                                        format!("![](<data:{media_type};base64,{data}>)"),
                                    ));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Render a `tool_use` block as markdown. Write/Edit become a code block
/// (language guessed from the file extension); Bash becomes a bash block;
/// everything else collapses to a one-line summary. The tool name + target are
/// kept as a language-neutral inline-code prefix; the localized category label
/// ("Tool call" / "工具调用") is rendered by the frontend from `kind`.
fn format_tool_use(block: &Value) -> String {
    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
    let input = block.get("input");
    match name {
        "Write" => {
            let path = input
                .and_then(|i| i.get("file_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let code = input
                .and_then(|i| i.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let lang = language_from_path(path);
            format!("`Write {path}`\n\n```{lang}\n{code}\n```")
        }
        "Edit" => {
            let path = input
                .and_then(|i| i.get("file_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let code = input
                .and_then(|i| i.get("new_string"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let lang = language_from_path(path);
            format!("`Edit {path}`\n\n```{lang}\n{code}\n```")
        }
        "Bash" => {
            let command = input
                .and_then(|i| i.get("command"))
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("`Bash`\n\n```bash\n{command}\n```")
        }
        _ => {
            let detail = match input {
                Some(Value::Object(m)) => m
                    .get("file_path")
                    .or_else(|| m.get("pattern"))
                    .or_else(|| m.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                _ => "",
            };
            if detail.is_empty() {
                format!("`{name}`")
            } else {
                format!("`{name} {detail}`")
            }
        }
    }
}

/// Render a `tool_result` block (diff / file content / command output) as a
/// plain code block. No language hint — the content type is ambiguous. The
/// localized "Tool result" label is added by the frontend from `kind`.
fn format_tool_result(block: &Value) -> String {
    let content = block.get("content").and_then(Value::as_str).unwrap_or("");
    format!("```\n{content}\n```")
}

/// Best-effort syntax language from a file extension, defaulting to plaintext.
fn language_from_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" => "bash",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "hh" => "cpp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "vue" => "vue",
        "svelte" => "svelte",
        "xml" => "xml",
        _ => "plaintext",
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
        let ts = value.get("timestamp").and_then(Value::as_str).map(String::from);
        let turn_uuid = value.get("uuid").and_then(Value::as_str).map(String::from);
        // Only assistant turns carry `message.model`; a session can switch
        // models mid-way (/model, or a subagent on a different one), so this
        // is read per turn rather than once for the file.
        let turn_model = message.get("model").and_then(Value::as_str).map(String::from);

        // Split the turn's content into per-block messages so tool calls,
        // results, and thinking are preserved (tagged by `kind`) and filterable.
        let blocks = message.get("content").map(extract_blocks).unwrap_or_default();
        if blocks.is_empty() {
            continue;
        }
        if created_at.is_none() {
            created_at = ts.clone();
        }
        if ts.is_some() {
            updated_at = ts.clone();
        }
        for (i, (kind, text)) in blocks.into_iter().enumerate() {
            // "meta" = CLI-injected local content riding on a user-role turn —
            // shown in the transcript but kept out of the human-input count/nav
            // and search role scoping. Tool output (tool_use/tool_result/thinking)
            // is agent-side work, so it's tagged "assistant", never "human".
            let sender = match kind.as_str() {
                "tool_use" | "tool_result" | "thinking" => "assistant".to_string(),
                _ if ty != "user" => "assistant".to_string(),
                _ if is_synthetic_local_wrapper(&text) => "meta".to_string(),
                _ => "human".to_string(),
            };
            let msg_id = turn_uuid
                .clone()
                .map(|u| format!("{u}_{i}"))
                .unwrap_or_else(|| format!("{session_id}_{}", messages.len()));
            let model = if sender == "assistant" { turn_model.clone() } else { None };
            messages.push(NormalizedMessage {
                id: msg_id,
                sender,
                text,
                created_at: ts.clone(),
                kind,
                model,
            });
        }
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
        summary: cwd.clone().unwrap_or_default(),
        cwd: cwd.unwrap_or_default(),
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
    // Codex records the model on the session/turn envelope rather than on the
    // message itself; `turn_context` is re-emitted whenever it changes, so we
    // carry the latest one forward onto the assistant turns that follow.
    let mut current_model: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let value: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = value.get("type").and_then(Value::as_str).unwrap_or("");
        let ts = value.get("timestamp").and_then(Value::as_str).map(String::from);

        if ty == "turn_context" {
            if let Some(m) = value
                .get("payload")
                .and_then(|p| p.get("model"))
                .and_then(Value::as_str)
            {
                current_model = Some(m.to_string());
            }
            continue;
        }

        if ty == "session_meta" {
            let payload = value.get("payload");
            // Keep the FIRST session_meta, not the last. A resumed session
            // writes a second one describing the session it forked from, and
            // that parent id is shared by every file resumed off it — taking
            // the last one collapsed 74 Codex sessions into 31 ids, so 43 were
            // lost and the survivors swapped content on every rescan. The first
            // entry is the file's own identity and matches its filename uuid.
            if session_id.is_none() {
                session_id = payload
                    .and_then(|p| p.get("id"))
                    .and_then(Value::as_str)
                    .map(String::from);
            }
            if cwd.is_none() {
                cwd = payload
                    .and_then(|p| p.get("cwd"))
                    .and_then(Value::as_str)
                    .map(String::from);
            }
            if current_model.is_none() {
                current_model = payload
                    .and_then(|p| p.get("model").or_else(|| {
                        p.get("collaboration_mode").and_then(|c| c.get("model"))
                    }))
                    .and_then(Value::as_str)
                    .map(String::from);
            }
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
        let blocks = payload.get("content").map(extract_blocks).unwrap_or_default();
        if blocks.is_empty() {
            continue;
        }
        if created_at.is_none() {
            created_at = ts.clone();
        }
        if ts.is_some() {
            updated_at = ts.clone();
        }
        let sid = session_id.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("codex")
                .to_string()
        });
        for (kind, text) in blocks {
            // Skip Codex's <environment_context>/<user_instructions> wrapper —
            // a synthetic local-context user turn, not a human message.
            if role == "user" && kind == "text" && is_synthetic_local_wrapper(&text) {
                continue;
            }
            let sender = match kind.as_str() {
                "tool_use" | "tool_result" | "thinking" => "assistant".to_string(),
                _ if role == "user" => "human".to_string(),
                _ => "assistant".to_string(),
            };
            let model = if sender == "assistant" { current_model.clone() } else { None };
            messages.push(NormalizedMessage {
                id: format!("{sid}_{}", messages.len()),
                sender,
                text,
                created_at: ts.clone(),
                kind,
                model,
            });
        }
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
        summary: cwd.clone().unwrap_or_default(),
        cwd: cwd.unwrap_or_default(),
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

    /// A resumed Codex session writes a second `session_meta` describing the
    /// session it forked from. Every file resumed off the same parent carries
    /// that same parent id, so keeping the last one made them all collide:
    /// on real data 74 sessions collapsed into 31 stored conversations, 43
    /// were lost outright, and each rescan swapped which one survived.
    #[test]
    fn a_resumed_codex_session_keeps_its_own_id_not_its_parents() {
        let dir = std::env::temp_dir().join(format!("codex_resume_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-2026-01-01T00-00-00-child-session.jsonl");
        let mut f = File::create(&path).unwrap();
        // This file's own identity comes first...
        writeln!(
            f,
            r#"{{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{{"id":"child-session","cwd":"/tmp/child"}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:01Z","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"hello"}}]}}}}"#
        ).unwrap();
        // ...and the parent it was resumed from comes after.
        writeln!(
            f,
            r#"{{"type":"session_meta","timestamp":"2026-01-01T00:00:02Z","payload":{{"id":"parent-session","cwd":"/tmp/parent"}}}}"#
        ).unwrap();
        drop(f);

        let conv = parse_codex_session(&path).expect("should parse");
        assert_eq!(conv.id, "codex:child-session", "the parent id would collide with its siblings");
        assert_eq!(conv.cwd, "/tmp/child", "cwd must describe this session, not the parent");

        let _ = std::fs::remove_dir_all(&dir);
    }

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

#[cfg(test)]
mod model_capture_tests {
    use super::*;
    use std::io::Write;

    /// A session can switch models mid-way (`/model`, or a subagent running on
    /// a different one), so the model is read per assistant turn — not once for
    /// the file — and human/meta turns carry none.
    #[test]
    fn claude_code_records_the_model_of_each_assistant_turn() {
        let dir = std::env::temp_dir().join(format!("cc_model_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","cwd":"/tmp/proj","timestamp":"2026-01-01T00:00:00Z","uuid":"u1","message":{{"content":"hi"}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","uuid":"a1","message":{{"model":"claude-sonnet-4-6","content":"hello"}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","uuid":"a2","message":{{"model":"claude-opus-5","content":"switched"}}}}"#
        ).unwrap();
        drop(f);

        let conv = parse_claude_code_session(&path).expect("should parse");
        let models: Vec<Option<&str>> = conv.messages.iter().map(|m| m.model.as_deref()).collect();
        assert_eq!(
            models,
            vec![None, Some("claude-sonnet-4-6"), Some("claude-opus-5")],
            "each assistant turn keeps its own model; the human turn has none"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex puts the model on the `turn_context` envelope rather than on the
    /// message, and re-emits it when it changes — it has to be carried forward
    /// onto the assistant turns that follow.
    #[test]
    fn codex_carries_the_turn_context_model_onto_following_turns() {
        let dir = std::env::temp_dir().join(format!("codex_model_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-test.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{{"id":"s1","cwd":"/tmp/proj"}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"turn_context","timestamp":"2026-01-01T00:00:01Z","payload":{{"model":"gpt-5.6-sol"}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:02Z","payload":{{"type":"message","role":"user","content":[{{"type":"input_text","text":"hi"}}]}}}}"#
        ).unwrap();
        writeln!(
            f,
            r#"{{"type":"response_item","timestamp":"2026-01-01T00:00:03Z","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"hello"}}]}}}}"#
        ).unwrap();
        drop(f);

        let conv = parse_codex_session(&path).expect("should parse");
        let models: Vec<Option<&str>> = conv.messages.iter().map(|m| m.model.as_deref()).collect();
        assert_eq!(models, vec![None, Some("gpt-5.6-sol")]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
