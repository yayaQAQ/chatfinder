//! A Model Context Protocol server, embedded in the app so it ships with it.
//!
//! Transport is streamable HTTP bound to loopback: it runs in-process, so it
//! reuses the same `DbState` connection every Tauri command uses and needs no
//! second binary in the bundle. The cost is that the server only exists while
//! the app is open, which is the deliberate trade.
//!
//! Everything exposed here is read-only. `PRAGMA query_only` is not set —
//! the connection is shared with the UI, which does write — so read-only is
//! enforced by only ever handing out SELECTs.

use crate::db::DbState;
use rusqlite::{params_from_iter, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

/// Protocol revisions we can speak. `initialize` echoes the client's version
/// when it is one of these and otherwise answers with our newest, which is what
/// the spec asks for.
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const SERVER_NAME: &str = "chatfinder";

/// A body larger than this is refused before allocating it. Tool arguments are
/// small; anything near this is either a bug or someone probing the port.
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Per-message cap applied by every tool that returns message bodies. A single
/// Claude Code tool_result can run to tens of thousands of characters, and a
/// handful of those is enough to fill an agent's whole context window.
const DEFAULT_MAX_CHARS: usize = 2000;

// ─── settings keys ──────────────────────────────────────────────────────────

const KEY_ENABLED: &str = "mcp_enabled";
const KEY_PORT: &str = "mcp_port";
const KEY_TOKEN: &str = "mcp_token";
pub const DEFAULT_PORT: u16 = 8722;

fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn put_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// 32 hex chars from two v4 UUIDs — `uuid` is already a dependency, and this
/// avoids pulling in an RNG crate just to mint a bearer token.
fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

// ─── server state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEntry {
    pub at: String,
    pub tool: String,
    pub detail: String,
    pub duration_ms: u64,
    pub ok: bool,
}

struct Running {
    port: u16,
    stop: Arc<AtomicBool>,
}

/// A short-lived permission for one local client to collect the bearer token
/// by itself.
///
/// The point is to keep the token out of the user's agent transcript. Handing
/// it over as text means it is written to `~/.claude/projects/**.jsonl`, which
/// this very app scans and indexes — the token would end up full-text
/// searchable inside the archive it guards. With enrollment the user pastes a
/// command containing no secret, the agent pipes the token straight from curl
/// into the client's config, and only `$TOKEN` is ever recorded.
///
/// Unauthenticated by necessity, so it is fenced in three ways: it exists only
/// after the user opens it, it expires, and the first fetch consumes it.
struct Enrollment {
    token: String,
    expires_at: std::time::Instant,
}

/// Long enough to paste a command and let an agent act on it, short enough that
/// an unauthenticated endpoint is not sitting open by accident.
const ENROLLMENT_TTL: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Default)]
pub struct McpState {
    running: Mutex<Option<Running>>,
    /// Last 100 tool calls. Not persisted — this exists so the settings panel
    /// can show what an agent actually did, which is the thing that makes
    /// leaving the port open feel accountable rather than opaque.
    activity: Mutex<VecDeque<ActivityEntry>>,
    last_error: Mutex<Option<String>>,
    enrollment: Mutex<Option<Enrollment>>,
}

impl McpState {
    fn log(&self, tool: &str, detail: String, duration_ms: u64, ok: bool) {
        if let Ok(mut q) = self.activity.lock() {
            if q.len() >= 100 {
                q.pop_front();
            }
            q.push_back(ActivityEntry {
                at: chrono::Utc::now().to_rfc3339(),
                tool: tool.to_string(),
                detail,
                duration_ms,
                ok,
            });
        }
    }
}

/// Hands out the token once, then closes the window. Returns `None` when no
/// window is open or it has expired, which is also when an expired one is
/// cleared away.
fn take_enrollment_token(app: &AppHandle) -> Option<String> {
    let state = app.state::<McpState>();
    let token = take_from_slot(&state.enrollment);
    if token.is_some() {
        state.log("enroll", "token collected by a local client".into(), 0, true);
    }
    token
}

/// The state machine, split from Tauri state so it can be tested directly.
fn take_from_slot(slot: &Mutex<Option<Enrollment>>) -> Option<String> {
    let mut guard = slot.lock().ok()?;
    let expired = guard
        .as_ref()
        .map(|e| e.expires_at <= std::time::Instant::now())
        .unwrap_or(false);
    if expired {
        *guard = None;
        return None;
    }
    // `take()` is the single-use part: a second fetch finds nothing.
    guard.take().map(|e| e.token)
}

fn enrollment_seconds_left(app: &AppHandle) -> u64 {
    let state = app.state::<McpState>();
    let guard = match state.enrollment.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard
        .as_ref()
        .map(|e| e.expires_at.saturating_duration_since(std::time::Instant::now()).as_secs())
        .unwrap_or(0)
}

// ─── path handling ──────────────────────────────────────────────────────────

/// How a stored working directory relates to the directory an agent asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rel {
    Exact,
    /// The stored directory contains the queried one — the agent is in a
    /// subdirectory of a project whose history was recorded at its root.
    Ancestor,
    /// The stored directory is inside the queried one — the agent is at a
    /// project root and history was recorded in submodules.
    Descendant,
}

impl Rel {
    fn as_str(self) -> &'static str {
        match self {
            Rel::Exact => "exact",
            Rel::Ancestor => "ancestor",
            Rel::Descendant => "descendant",
        }
    }
}

/// Ancestor matching stops here. `/Users/alice` is an ancestor of every project
/// a person owns, so treating it as "related" would make the filter meaningless;
/// requiring three components keeps `/Users/alice/Code/proj` and rejects `/Users`
/// and `/Users/alice`. Counted in *meaningful* components — see `depth`.
const MIN_ANCESTOR_COMPONENTS: usize = 3;

/// Both separators are accepted everywhere: a path can reach this code from the
/// host filesystem or from a session recorded on another machine, so the shape
/// of the string is not a reliable guide to the OS it came from.
const SEPARATORS: [char; 2] = ['/', '\\'];

/// `c:` — a Windows drive letter, which is structural rather than a directory
/// anyone chose, so it is excluded from the depth requirement.
fn is_drive(component: &str) -> bool {
    let b = component.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

pub fn normalize_path(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let is_home_relative = trimmed == "~"
        || trimmed.strip_prefix('~').is_some_and(|r| r.starts_with(SEPARATORS));
    let expanded = if is_home_relative {
        match dirs::home_dir() {
            Some(home) => home.join(trimmed.trim_start_matches('~').trim_start_matches(SEPARATORS)),
            None => std::path::PathBuf::from(trimmed),
        }
    } else {
        std::path::PathBuf::from(trimmed)
    };
    // Resolve symlinks when the directory exists — on macOS `/tmp` is really
    // `/private/tmp`, and an agent's cwd and a recorded session cwd can spell
    // the same place differently. A path that no longer exists is kept as-is.
    let resolved = std::fs::canonicalize(&expanded).unwrap_or(expanded);
    let s = resolved.to_string_lossy().to_string();
    // Windows `canonicalize` hands back an extended-length path (`\\?\C:\x`);
    // the agent transcripts record the plain form, so strip the prefix or
    // nothing would ever match.
    let s = match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => s.strip_prefix(r"\\?\").unwrap_or(&s).to_string(),
    };
    let s = s.trim_end_matches(SEPARATORS).to_string();
    // A bare root trims away to nothing on POSIX; `C:` is already the root.
    if s.is_empty() { "/".to_string() } else { s }
}

fn components(path: &str) -> Vec<String> {
    path.trim_matches(SEPARATORS)
        .split(SEPARATORS)
        .filter(|c| !c.is_empty())
        // APFS is case-insensitive by default (so is NTFS), so `/users/a/Proj`
        // and `/Users/a/proj` are the same directory.
        .map(|c| c.to_lowercase())
        .collect()
}

/// How many components a person actually chose. Dropping a leading drive letter
/// keeps `C:\Users\alice` as shallow as `/Users/alice`, so the ancestor cutoff
/// means the same thing on both platforms.
fn depth(comps: &[String]) -> usize {
    match comps.first() {
        Some(first) if is_drive(first) => comps.len() - 1,
        _ => comps.len(),
    }
}

pub fn relation(query: &str, stored: &str) -> Option<Rel> {
    let q = components(query);
    let s = components(stored);
    if q.is_empty() || s.is_empty() {
        return None;
    }
    if q == s {
        return Some(Rel::Exact);
    }
    if q.len() > s.len() && q[..s.len()] == s[..] {
        return if depth(&s) >= MIN_ANCESTOR_COMPONENTS { Some(Rel::Ancestor) } else { None };
    }
    if s.len() > q.len() && s[..q.len()] == q[..] {
        return if depth(&q) >= MIN_ANCESTOR_COMPONENTS { Some(Rel::Descendant) } else { None };
    }
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct DirRow {
    pub path: String,
    pub conversations: i64,
    pub messages: i64,
    pub first_active: Option<String>,
    pub last_active: Option<String>,
    pub platforms: Vec<String>,
}

/// Every distinct working directory, with counts. Small by construction — one
/// row per project a person has run an agent in — so it is loaded whole and
/// matched in Rust rather than with SQL prefix gymnastics.
fn load_directories(conn: &Connection) -> Result<Vec<DirRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT cwd,
                    COUNT(*),
                    COALESCE(SUM(message_count), 0),
                    MIN(DATE(COALESCE(created_at, imported_at))),
                    MAX(DATE(COALESCE(updated_at, created_at, imported_at))),
                    GROUP_CONCAT(DISTINCT platform)
             FROM conversations
             WHERE cwd != ''
             GROUP BY cwd",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let platforms: Option<String> = row.get(5)?;
            Ok(DirRow {
                path: row.get(0)?,
                conversations: row.get(1)?,
                messages: row.get(2)?,
                first_active: row.get(3)?,
                last_active: row.get(4)?,
                platforms: platforms
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    // Recency, not volume: an agent asking "what happened here" cares about the
    // directories that are live, not the one that historically talked most.
    out.sort_by(|a, b| b.last_active.cmp(&a.last_active).then(a.path.cmp(&b.path)));
    Ok(out)
}

/// Directories related to `cwd`, best relation first.
fn matching_dirs(dirs: &[DirRow], cwd: &str, scope: &str) -> Vec<(DirRow, Rel)> {
    let normalized = normalize_path(cwd);
    let mut out: Vec<(DirRow, Rel)> = dirs
        .iter()
        .filter_map(|d| {
            relation(&normalized, &d.path).and_then(|rel| match scope {
                "exact" if rel != Rel::Exact => None,
                _ => Some((d.clone(), rel)),
            })
        })
        .collect();
    out.sort_by_key(|(d, rel)| {
        (
            match rel {
                Rel::Exact => 0,
                Rel::Ancestor => 1,
                Rel::Descendant => 2,
            },
            std::cmp::Reverse(d.last_active.clone()),
        )
    });
    out
}

// ─── argument helpers ───────────────────────────────────────────────────────

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn arg_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

fn arg_strs(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn clamp(v: i64, lo: i64, hi: i64) -> i64 {
    v.max(lo).min(hi)
}

/// Cuts a message body down to `max` characters on a char boundary and says how
/// much was dropped, so the agent knows to fetch the rest rather than assuming
/// it read the whole thing.
fn truncate(text: &str, max: usize) -> (String, bool) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return (text.to_string(), false);
    }
    let head: String = chars[..max].iter().collect();
    (
        format!("{head}\n… [truncated, {} more characters]", chars.len() - max),
        true,
    )
}

// ─── shared conversation filters ────────────────────────────────────────────

type SqlValue = rusqlite::types::Value;

struct Filters {
    sql: Vec<String>,
    params: Vec<SqlValue>,
    matched_dirs: Vec<(DirRow, Rel)>,
    /// Set when a `cwd` was given, describing what it resolved to. Surfaced in
    /// the response so the agent can tell "no results here" from "this
    /// directory has no recorded history at all".
    scope_note: Option<String>,
}

fn build_filters(conn: &Connection, args: &Value, alias: &str) -> Result<Filters, String> {
    let mut f = Filters { sql: Vec::new(), params: Vec::new(), matched_dirs: Vec::new(), scope_note: None };
    let a = alias;

    if let Some(p) = arg_str(args, "platform") {
        f.sql.push(format!("{a}.platform = ?"));
        f.params.push(SqlValue::Text(p));
    }
    if let Some(m) = arg_str(args, "model") {
        // `models` is a comma-joined list, so match a whole element rather than
        // a substring — "gpt-5" must not match "gpt-5.6-sol".
        f.sql.push(format!("INSTR(',' || {a}.models || ',', ',' || ? || ',') > 0"));
        f.params.push(SqlValue::Text(m));
    }
    if let Some(d) = arg_str(args, "date_from") {
        f.sql.push(format!("DATE(COALESCE({a}.updated_at, {a}.created_at, {a}.imported_at)) >= ?"));
        f.params.push(SqlValue::Text(d));
    }
    if let Some(d) = arg_str(args, "date_to") {
        f.sql.push(format!("DATE(COALESCE({a}.updated_at, {a}.created_at, {a}.imported_at)) <= ?"));
        f.params.push(SqlValue::Text(d));
    }
    if let Some(n) = arg_i64(args, "min_messages") {
        f.sql.push(format!("{a}.message_count >= ?"));
        f.params.push(SqlValue::Integer(n));
    }
    if let Some(n) = arg_i64(args, "max_messages") {
        f.sql.push(format!("{a}.message_count <= ?"));
        f.params.push(SqlValue::Integer(n));
    }

    if let Some(cwd) = arg_str(args, "cwd") {
        let scope = arg_str(args, "cwd_scope").unwrap_or_else(|| "tree".to_string());
        if scope != "off" {
            let dirs = load_directories(conn)?;
            let matched = matching_dirs(&dirs, &cwd, &scope);
            if matched.is_empty() {
                // An impossible predicate rather than an empty IN (), which is a
                // syntax error in SQLite.
                f.sql.push("1 = 0".to_string());
                f.scope_note = Some(format!(
                    "No recorded working directory matches {}. Agent sessions cover only part of the library — try search without cwd, using the project name as a keyword.",
                    normalize_path(&cwd)
                ));
            } else {
                let placeholders = vec!["?"; matched.len()].join(", ");
                f.sql.push(format!("{a}.cwd IN ({placeholders})"));
                for (d, _) in &matched {
                    f.params.push(SqlValue::Text(d.path.clone()));
                }
                f.scope_note = Some(format!(
                    "Scoped to {} directory/directories: {}",
                    matched.len(),
                    matched
                        .iter()
                        .map(|(d, r)| format!("{} ({})", d.path, r.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                f.matched_dirs = matched;
            }
        }
    }
    Ok(f)
}

fn where_clause(f: &Filters, extra: &[String]) -> String {
    let mut all: Vec<String> = f.sql.clone();
    all.extend(extra.iter().cloned());
    if all.is_empty() {
        String::new()
    } else {
        format!(" AND {}", all.join(" AND "))
    }
}

// ─── tools ──────────────────────────────────────────────────────────────────

fn tool_overview(conn: &Connection, args: &Value) -> Result<Value, String> {
    let (conversations, messages): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(message_count), 0) FROM conversations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare("SELECT platform, COUNT(*) FROM conversations GROUP BY platform ORDER BY 2 DESC")
        .map_err(|e| e.to_string())?;
    let mut platforms = serde_json::Map::new();
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (p, n) = row.map_err(|e| e.to_string())?;
        platforms.insert(p, json!(n));
    }

    let (first, last): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT MIN(DATE(COALESCE(created_at, imported_at))),
                    MAX(DATE(COALESCE(updated_at, created_at, imported_at)))
             FROM conversations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    let dirs = load_directories(conn)?;
    let with_dir: i64 = dirs.iter().map(|d| d.conversations).sum();
    let without_dir = conversations - with_dir;

    let dir_limit = clamp(arg_i64(args, "directory_limit").unwrap_or(30), 1, 200) as usize;

    let current = arg_str(args, "cwd").map(|cwd| {
        let normalized = normalize_path(&cwd);
        let matched = matching_dirs(&dirs, &cwd, "tree");
        // The keyword worth suggesting is the project's name, not the leaf the
        // agent happens to be standing in — "src" finds nothing useful.
        let basename = matched
            .iter()
            .find(|(_, r)| *r != Rel::Descendant)
            .map(|(d, _)| d.path.clone())
            .unwrap_or_else(|| normalized.clone())
            .trim_end_matches(SEPARATORS)
            .rsplit(SEPARATORS)
            .next()
            .unwrap_or("")
            .to_string();
        let pick = |rel: Rel| -> Vec<&DirRow> {
            matched.iter().filter(|(_, r)| *r == rel).map(|(d, _)| d).collect()
        };
        json!({
            "queried": cwd,
            "normalized": normalized,
            "exact": pick(Rel::Exact).first().map(|d| json!(d)),
            "ancestors": pick(Rel::Ancestor),
            "descendants": pick(Rel::Descendant),
            "hint": format!(
                "{} conversation(s) have no working directory at all (web exports from ChatGPT/Claude/DeepSeek). \
                 They can still be about this project — search them with \"{}\" as a keyword and cwd omitted.",
                without_dir, basename
            ),
        })
    });

    Ok(json!({
        "totals": { "conversations": conversations, "messages": messages },
        "platforms": platforms,
        "date_range": { "first": first, "last": last },
        "current_directory": current,
        "directories": dirs.iter().take(dir_limit).collect::<Vec<_>>(),
        "directory_coverage": {
            "conversations_with_directory": with_dir,
            "conversations_without_directory": without_dir,
            "distinct_directories": dirs.len(),
            "directories_shown": dirs.len().min(dir_limit),
        },
    }))
}

fn tool_list_directories(conn: &Connection, args: &Value) -> Result<Value, String> {
    let mut dirs = load_directories(conn)?;
    match arg_str(args, "sort").unwrap_or_else(|| "recent".into()).as_str() {
        "conversations" => dirs.sort_by(|a, b| b.conversations.cmp(&a.conversations)),
        "path" => dirs.sort_by(|a, b| a.path.cmp(&b.path)),
        _ => {}
    }
    let total = dirs.len();
    let offset = clamp(arg_i64(args, "offset").unwrap_or(0), 0, i64::MAX) as usize;
    let limit = clamp(arg_i64(args, "limit").unwrap_or(50), 1, 500) as usize;
    let page: Vec<&DirRow> = dirs.iter().skip(offset).take(limit).collect();
    Ok(json!({
        "directories": page,
        "total": total,
        "offset": offset,
        "has_more": offset + page.len() < total,
    }))
}

fn tool_list_conversations(conn: &Connection, args: &Value) -> Result<Value, String> {
    let f = build_filters(conn, args, "c")?;
    let limit = clamp(arg_i64(args, "limit").unwrap_or(20), 1, 100);
    let offset = clamp(arg_i64(args, "offset").unwrap_or(0), 0, i64::MAX);
    let order = match arg_str(args, "sort").unwrap_or_else(|| "newest".into()).as_str() {
        "oldest" => "COALESCE(c.updated_at, c.created_at) ASC",
        "most_messages" => "c.message_count DESC",
        _ => "COALESCE(c.updated_at, c.created_at) DESC",
    };
    let sql = format!(
        "SELECT c.id, c.platform, c.title, c.created_at, c.updated_at, c.message_count, c.models, c.cwd
         FROM conversations c
         WHERE 1 = 1{}
         ORDER BY {order}
         LIMIT ? OFFSET ?",
        where_clause(&f, &[])
    );
    let mut params = f.params.clone();
    params.push(SqlValue::Integer(limit + 1)); // one extra row answers has_more
    params.push(SqlValue::Integer(offset));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |r| {
            let models: String = r.get(6)?;
            Ok(json!({
                "id": r.get::<_, String>(0)?,
                "platform": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "created_at": r.get::<_, Option<String>>(3)?,
                "updated_at": r.get::<_, Option<String>>(4)?,
                "message_count": r.get::<_, i64>(5)?,
                "models": models.split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
                "cwd": r.get::<_, String>(7)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let mut out: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let has_more = out.len() as i64 > limit;
    out.truncate(limit as usize);

    Ok(json!({
        "conversations": out,
        "returned": out.len(),
        "offset": offset,
        "has_more": has_more,
        "scope": f.scope_note,
    }))
}

/// Builds a `…before【match】after…` window around the first occurrence, mirroring
/// what FTS5's `snippet()` produces so both search modes read the same.
fn make_snippet(text: &str, needle: &str, radius: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let lower_text = text.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let pos = lower_text
        .find(&lower_needle)
        .map(|b| lower_text[..b].chars().count())
        .unwrap_or(0)
        .min(chars.len());
    let n_len = needle.chars().count().min(chars.len() - pos);
    let start = pos.saturating_sub(radius);
    let end = (pos + n_len + radius).min(chars.len());
    let take = |r: std::ops::Range<usize>| -> String { chars[r].iter().collect() };
    format!(
        "{}{}【{}】{}{}",
        if start > 0 { "…" } else { "" },
        take(start..pos),
        take(pos..pos + n_len),
        take(pos + n_len..end),
        if end < chars.len() { "…" } else { "" },
    )
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// True when the query is one unbroken run of CJK. FTS5's default `unicode61`
/// tokenizer does not segment Chinese, so such a run becomes a single token and
/// only prefix matches can ever hit — "方案" cannot find "重构方案". Those
/// queries go straight to a substring scan instead of quietly returning nothing.
fn is_cjk_query(q: &str) -> bool {
    q.chars().any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF))
}


/// True when the marked term sits inside a long unbroken run of base64-ish
/// characters — a match inside an encoded blob rather than in prose.
///
/// Some imported conversations carry raw base64 (images, keys, build output)
/// in message text. FTS5's tokenizer splits those on `+` and `/` into
/// pronounceable-looking fragments, so a short query can match one, and bm25
/// ranks it above real text. The run is measured outward from the match, so a
/// message that merely *contains* a long token elsewhere still ranks normally.
/// English prose has spaces and CJK is outside the character class, so neither
/// can trip this.
fn match_sits_in_an_encoded_blob(snippet: &str) -> bool {
    const RUN_LIMIT: usize = 60;
    let is_blob_char = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=';

    let Some(open) = snippet.find('\u{3010}') else { return false };
    let Some(close) = snippet.find('\u{3011}') else { return false };
    if close < open {
        return false;
    }
    let term = &snippet[open + '\u{3010}'.len_utf8()..close];
    if !term.chars().all(is_blob_char) {
        return false;
    }

    let before = snippet[..open].chars().rev().take_while(|c| is_blob_char(*c)).count();
    let after = snippet[close + '\u{3011}'.len_utf8()..]
        .chars()
        .take_while(|c| is_blob_char(*c))
        .count();
    before + term.chars().count() + after >= RUN_LIMIT
}

fn search_fts(
    conn: &Connection,
    fts_query: &str,
    f: &Filters,
    extra_sql: &[String],
    extra_params: &[SqlValue],
    limit: i64,
) -> Result<Vec<Value>, String> {
    let sql = format!(
        "SELECT si.ref_id, si.conversation_id, si.kind,
                snippet(search_index, 3, '【', '】', '…', 16),
                c.title, c.platform, c.updated_at, c.created_at, c.cwd, m.sender, m.kind
         FROM search_index si
         JOIN conversations c ON c.id = si.conversation_id
         LEFT JOIN messages m ON m.id = si.ref_id
         WHERE si.text MATCH ?{}
         ORDER BY bm25(search_index) ASC
         LIMIT 400",
        where_clause(f, extra_sql)
    );
    let mut params = vec![SqlValue::Text(fts_query.to_string())];
    params.extend(f.params.iter().cloned());
    params.extend(extra_params.iter().cloned());

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // One hit per conversation (best bm25 kept), matching the app's own search:
    // otherwise a single chatty session floods the whole result set.
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let conversation_id: String = row.get(1).map_err(|e| e.to_string())?;
        if !seen.insert(conversation_id.clone()) {
            continue;
        }
        let ref_id: String = row.get(0).map_err(|e| e.to_string())?;
        let hit_in: String = row.get(2).map_err(|e| e.to_string())?;
        let snippet: String = row.get(3).map_err(|e| e.to_string())?;
        if match_sits_in_an_encoded_blob(&snippet) {
            // Undo the dedup claim so a genuine later hit in the same
            // conversation can still be returned.
            seen.remove(&conversation_id);
            continue;
        }
        hits.push(json!({
            "conversation_id": conversation_id,
            "conversation_title": row.get::<_, String>(4).map_err(|e| e.to_string())?,
            "platform": row.get::<_, String>(5).map_err(|e| e.to_string())?,
            "cwd": row.get::<_, String>(8).map_err(|e| e.to_string())?,
            "hit_in": hit_in.clone(),
            "message_id": if hit_in == "message" { Some(ref_id) } else { None },
            "sender": row.get::<_, Option<String>>(9).map_err(|e| e.to_string())?,
            "message_kind": row.get::<_, Option<String>>(10).map_err(|e| e.to_string())?,
            "snippet": snippet,
            "updated_at": row.get::<_, Option<String>>(6).map_err(|e| e.to_string())?,
            "created_at": row.get::<_, Option<String>>(7).map_err(|e| e.to_string())?,
        }));
        if hits.len() as i64 >= limit {
            break;
        }
    }
    Ok(hits)
}

fn search_substring(
    conn: &Connection,
    query: &str,
    f: &Filters,
    extra_sql: &[String],
    extra_params: &[SqlValue],
    limit: i64,
) -> Result<Vec<Value>, String> {
    let sql = format!(
        "SELECT m.id, m.conversation_id, m.text, m.sender, m.kind,
                c.title, c.platform, c.updated_at, c.created_at, c.cwd
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE m.text LIKE ? ESCAPE '\\'{}
         ORDER BY COALESCE(c.updated_at, c.created_at) DESC
         LIMIT 400",
        where_clause(f, extra_sql)
    );
    let mut params = vec![SqlValue::Text(format!("%{}%", escape_like(query)))];
    params.extend(f.params.iter().cloned());
    params.extend(extra_params.iter().cloned());

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let conversation_id: String = row.get(1).map_err(|e| e.to_string())?;
        if !seen.insert(conversation_id.clone()) {
            continue;
        }
        let text: String = row.get(2).map_err(|e| e.to_string())?;
        let snippet = make_snippet(&text, query, 60);
        if match_sits_in_an_encoded_blob(&snippet) {
            seen.remove(&conversation_id);
            continue;
        }
        hits.push(json!({
            "conversation_id": conversation_id,
            "conversation_title": row.get::<_, String>(5).map_err(|e| e.to_string())?,
            "platform": row.get::<_, String>(6).map_err(|e| e.to_string())?,
            "cwd": row.get::<_, String>(9).map_err(|e| e.to_string())?,
            "hit_in": "message",
            "message_id": row.get::<_, String>(0).map_err(|e| e.to_string())?,
            "sender": row.get::<_, String>(3).map_err(|e| e.to_string())?,
            "message_kind": row.get::<_, String>(4).map_err(|e| e.to_string())?,
            "snippet": snippet,
            "updated_at": row.get::<_, Option<String>>(7).map_err(|e| e.to_string())?,
            "created_at": row.get::<_, Option<String>>(8).map_err(|e| e.to_string())?,
        }));
        if hits.len() as i64 >= limit {
            break;
        }
    }
    Ok(hits)
}

/// Whole-word match first, prefix match only if that finds nothing.
///
/// The app's own search always appends `*`, which is right for a search box
/// where someone is still typing. Here it is wrong: an agent sends complete
/// words, and on this data a prefix query for a short acronym also matches
/// fragments of base64 blobs that happen to start with the same letters, which
/// bm25 then ranks above real prose. Returns whether the prefix form was used.
fn search_fts_with_fallback(
    conn: &Connection,
    query: &str,
    f: &Filters,
    extra_sql: &[String],
    extra_params: &[SqlValue],
    limit: i64,
) -> Result<(Vec<Value>, bool), String> {
    let cleaned = query.replace('"', " ");
    let exact = search_fts(conn, &cleaned, f, extra_sql, extra_params, limit)?;
    if !exact.is_empty() {
        return Ok((exact, false));
    }
    let prefixed = search_fts(conn, &format!("{cleaned}*"), f, extra_sql, extra_params, limit)?;
    Ok((prefixed, true))
}

fn tool_search(conn: &Connection, args: &Value) -> Result<Value, String> {
    let query = arg_str(args, "query").ok_or("`query` is required")?;
    let limit = clamp(arg_i64(args, "limit").unwrap_or(20), 1, 100);
    let f = build_filters(conn, args, "c")?;

    // sender/kind live on `messages`, which both search paths already join.
    let mut extra_sql = Vec::new();
    let mut extra_params = Vec::new();
    if let Some(s) = arg_str(args, "sender") {
        extra_sql.push("m.sender = ?".to_string());
        extra_params.push(SqlValue::Text(s));
    }
    if let Some(k) = arg_str(args, "message_kind") {
        extra_sql.push("m.kind = ?".to_string());
        extra_params.push(SqlValue::Text(k));
    }

    let requested = arg_str(args, "mode").unwrap_or_else(|| "auto".into());
    let mut mode_used = requested.clone();
    let mut notes: Vec<String> = Vec::new();

    let hits = match requested.as_str() {
        "substring" => search_substring(conn, &query, &f, &extra_sql, &extra_params, limit)?,
        "fts" => search_fts_with_fallback(conn, &query, &f, &extra_sql, &extra_params, limit)?.0,
        _ => {
            if is_cjk_query(&query) {
                mode_used = "substring".into();
                notes.push(
                    "Chinese query: FTS5 cannot segment CJK, so a substring scan was used instead."
                        .to_string(),
                );
                search_substring(conn, &query, &f, &extra_sql, &extra_params, limit)?
            } else {
                // A query carrying FTS5 operators can fail to parse; that is a
                // reason to fall back, not to hand the agent a syntax error.
                match search_fts_with_fallback(conn, &query, &f, &extra_sql, &extra_params, limit) {
                    Ok((hits, prefixed)) if !hits.is_empty() => {
                        mode_used = if prefixed { "fts_prefix".into() } else { "fts".into() };
                        hits
                    }
                    other => {
                        if other.is_err() {
                            notes.push("Full-text query could not be parsed; fell back to substring.".into());
                        } else {
                            notes.push("Full-text search found nothing; fell back to substring.".into());
                        }
                        mode_used = "substring".into();
                        search_substring(conn, &query, &f, &extra_sql, &extra_params, limit)?
                    }
                }
            }
        }
    };

    if let Some(note) = &f.scope_note {
        notes.push(note.clone());
    }
    Ok(json!({
        "query": query,
        "mode_used": mode_used,
        "hits": hits,
        "returned": hits.len(),
        "notes": notes,
        "next_step": "Use get_message_context with a message_id to read around a hit, or get_conversation for the whole thread.",
    }))
}

/// Turns whatever an agent is holding into conversation ids.
///
/// A Claude Code or Codex session is stored as `cc:<uuid>` / `codex:<uuid>`,
/// but an agent knows its session by the bare uuid — that is what the CLI
/// prints, what `--resume` takes, and what names the transcript file. Requiring
/// the stored form would mean the caller has to know a storage detail, so the
/// bare uuid, a full path to the transcript, and the stored id all resolve.
///
/// Returns every candidate: a prefix can legitimately match more than one.
fn resolve_conversation_ids(conn: &Connection, input: &str) -> Result<Vec<String>, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("an id is required".into());
    }
    // `~/.claude/projects/<slug>/<uuid>.jsonl` → `<uuid>`
    let candidate = raw
        .rsplit(SEPARATORS)
        .next()
        .unwrap_or(raw)
        .trim_end_matches(".jsonl")
        .trim_start_matches("rollout-");

    let exists = |id: &str| -> bool {
        conn.query_row("SELECT 1 FROM conversations WHERE id = ?1", [id], |_| Ok(()))
            .is_ok()
    };
    if exists(candidate) {
        return Ok(vec![candidate.to_string()]);
    }
    for prefix in ["cc:", "codex:"] {
        let prefixed = format!("{prefix}{candidate}");
        if exists(&prefixed) {
            return Ok(vec![prefixed]);
        }
    }

    // Nothing matched outright — fall back to a prefix search, which covers the
    // shortened ids that show up in logs and terminal output.
    let mut stmt = conn
        .prepare(
            "SELECT id FROM conversations
             WHERE id = ?1 OR id LIKE 'cc:' || ?1 || '%' OR id LIKE 'codex:' || ?1 || '%'
                OR id LIKE ?1 || '%'
             ORDER BY id LIMIT 10",
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map([candidate], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Resolves to exactly one id, turning "none" and "several" into messages an
/// agent can act on rather than a bare failure.
fn resolve_one(conn: &Connection, input: &str) -> Result<String, String> {
    let ids = resolve_conversation_ids(conn, input)?;
    match ids.len() {
        1 => Ok(ids.into_iter().next().unwrap()),
        0 => Err(format!(
            "no imported conversation matches \"{input}\". Agent sessions only appear after ChatFinder scans them — a session that is still running, or that started since the last scan, will not be here yet."
        )),
        _ => Err(format!(
            "\"{input}\" matches several conversations: {}. Pass the full id.",
            ids.join(", ")
        )),
    }
}

/// The session id an agent would recognise, plus the command that reopens it.
fn session_fields(id: &str, platform: &str, cwd: &str) -> (Option<String>, Option<String>) {
    let Some(session_id) = (match platform {
        "claude-code" => id.strip_prefix("cc:"),
        "codex" => id.strip_prefix("codex:"),
        // ZIP-imported web chats have no session and cannot be resumed.
        _ => None,
    }) else {
        return (None, None);
    };
    let command = match platform {
        "claude-code" => format!("claude --resume {session_id}"),
        _ => format!("codex resume {session_id}"),
    };
    // The working directory matters: resuming elsewhere gives the session a
    // different project context than it ran in.
    let command = if cwd.is_empty() { command } else { format!("cd {cwd} && {command}") };
    (Some(session_id.to_string()), Some(command))
}

fn conversation_header(conn: &Connection, id: &str) -> Result<Value, String> {
    conn.query_row(
        "SELECT id, platform, title, url, created_at, updated_at, message_count, models, cwd, imported_at
         FROM conversations WHERE id = ?1",
        [id],
        |r| {
            let models: String = r.get(7)?;
            let id: String = r.get(0)?;
            let platform: String = r.get(1)?;
            let cwd: String = r.get(8)?;
            let (session_id, resume_command) = session_fields(&id, &platform, &cwd);
            let last_message_at: Option<String> = r.get(5)?;
            let imported_at: String = r.get(9)?;
            let message_count: i64 = r.get(6)?;
            Ok(json!({
                "id": id,
                "platform": platform,
                "title": r.get::<_, String>(2)?,
                "url": r.get::<_, Option<String>>(3)?,
                "created_at": r.get::<_, Option<String>>(4)?,
                "updated_at": last_message_at,
                "message_count": message_count,
                "models": models.split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
                "cwd": cwd,
                "session_id": session_id,
                "resume_command": resume_command,
                // Imports are snapshots taken by a scan, not a live mirror. Saying
                // so inline is the difference between an agent knowing it has
                // partial history and it assuming this is the whole conversation.
                "snapshot": {
                    "imported_at": imported_at,
                    "last_message_at": last_message_at,
                    "message_count": message_count,
                    "note": "Captured when ChatFinder last scanned agent sessions. Anything said after imported_at — including the rest of a session that is still running — is not here until the next scan.",
                },
            }))
        },
    )
    .map_err(|_| format!("no conversation with id {id}"))
}

fn read_messages(
    conn: &Connection,
    sql: &str,
    params: Vec<SqlValue>,
    max_chars: usize,
) -> Result<(Vec<Value>, usize), String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params_from_iter(params.iter())).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut truncated = 0usize;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let text: String = row.get(2).map_err(|e| e.to_string())?;
        let (text, was_cut) = truncate(&text, max_chars);
        if was_cut {
            truncated += 1;
        }
        out.push(json!({
            "id": row.get::<_, String>(0).map_err(|e| e.to_string())?,
            "seq": row.get::<_, i64>(1).map_err(|e| e.to_string())?,
            "text": text,
            "sender": row.get::<_, String>(3).map_err(|e| e.to_string())?,
            "kind": row.get::<_, String>(4).map_err(|e| e.to_string())?,
            "model": row.get::<_, Option<String>>(5).map_err(|e| e.to_string())?,
            "created_at": row.get::<_, Option<String>>(6).map_err(|e| e.to_string())?,
        }));
    }
    Ok((out, truncated))
}

fn tool_get_conversation(conn: &Connection, args: &Value) -> Result<Value, String> {
    let id = resolve_one(conn, &arg_str(args, "id").ok_or("`id` is required")?)?;
    let header = conversation_header(conn, &id)?;
    let offset = clamp(arg_i64(args, "offset").unwrap_or(0), 0, i64::MAX);
    let limit = clamp(arg_i64(args, "limit").unwrap_or(50), 1, 200);
    let max_chars = clamp(arg_i64(args, "max_chars_per_message").unwrap_or(DEFAULT_MAX_CHARS as i64), 100, 20000) as usize;

    let kinds = arg_strs(args, "kinds");
    let (kind_sql, kind_params) = if kinds.is_empty() {
        (String::new(), Vec::new())
    } else {
        (
            format!(" AND kind IN ({})", vec!["?"; kinds.len()].join(", ")),
            kinds.iter().map(|k| SqlValue::Text(k.clone())).collect::<Vec<_>>(),
        )
    };

    let total: i64 = {
        let sql = format!("SELECT COUNT(*) FROM messages WHERE conversation_id = ?{kind_sql}");
        let mut p = vec![SqlValue::Text(id.clone())];
        p.extend(kind_params.iter().cloned());
        conn.query_row(&sql, params_from_iter(p.iter()), |r| r.get(0))
            .map_err(|e| e.to_string())?
    };

    let sql = format!(
        "SELECT id, seq, text, sender, kind, model, created_at
         FROM messages WHERE conversation_id = ?{kind_sql}
         ORDER BY seq ASC LIMIT ? OFFSET ?"
    );
    let mut p = vec![SqlValue::Text(id.clone())];
    p.extend(kind_params);
    p.push(SqlValue::Integer(limit));
    p.push(SqlValue::Integer(offset));
    let (messages, truncated) = read_messages(conn, &sql, p, max_chars)?;

    let returned = messages.len() as i64;
    Ok(json!({
        "conversation": header,
        "messages": messages,
        "pagination": {
            "offset": offset,
            "limit": limit,
            "returned": returned,
            "total_matching": total,
            "has_more": offset + returned < total,
        },
        "truncated_messages": truncated,
        "hint": "Agent sessions are mostly tool_use/tool_result. Pass kinds:[\"text\"] to read just the conversation.",
    }))
}

/// Session id → conversation, without pulling any messages.
///
/// The cheap probe an agent makes before deciding whether to read: does this
/// session exist in the archive, how much of it was captured, and how stale is
/// that capture.
fn tool_find_session(conn: &Connection, args: &Value) -> Result<Value, String> {
    let input = arg_str(args, "session_id").ok_or("`session_id` is required")?;
    let ids = resolve_conversation_ids(conn, &input)?;
    if ids.is_empty() {
        return Ok(json!({
            "query": input,
            "matches": [],
            "note": "No imported conversation matches that id. Agent sessions appear only after ChatFinder scans them, so a session that is still running — or that started since the last scan — will not be here yet.",
        }));
    }
    let matches = ids
        .iter()
        .map(|id| conversation_header(conn, id))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "query": input,
        "matches": matches,
        "returned": matches.len(),
        "next_step": "Pass the `id` of a match to get_conversation to read it.",
    }))
}

fn tool_get_message_context(conn: &Connection, args: &Value) -> Result<Value, String> {
    let message_id = arg_str(args, "message_id").ok_or("`message_id` is required")?;
    let before = clamp(arg_i64(args, "before").unwrap_or(3), 0, 50);
    let after = clamp(arg_i64(args, "after").unwrap_or(3), 0, 50);
    let max_chars = clamp(arg_i64(args, "max_chars_per_message").unwrap_or(DEFAULT_MAX_CHARS as i64), 100, 20000) as usize;

    let (conversation_id, seq): (String, i64) = conn
        .query_row(
            "SELECT conversation_id, seq FROM messages WHERE id = ?1",
            [&message_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| format!("no message with id {message_id}"))?;

    let sql = "SELECT id, seq, text, sender, kind, model, created_at
               FROM messages WHERE conversation_id = ? AND seq BETWEEN ? AND ?
               ORDER BY seq ASC";
    let params = vec![
        SqlValue::Text(conversation_id.clone()),
        SqlValue::Integer(seq - before),
        SqlValue::Integer(seq + after),
    ];
    let (messages, truncated) = read_messages(conn, sql, params, max_chars)?;

    Ok(json!({
        "conversation": conversation_header(conn, &conversation_id)?,
        "target_message_id": message_id,
        "target_seq": seq,
        "messages": messages,
        "truncated_messages": truncated,
    }))
}

// ─── tool registry ──────────────────────────────────────────────────────────

fn tool_definitions() -> Value {
    json!([
        {
            "name": "overview",
            "description": "Start here. Totals, per-platform counts, date range, and the working directories that agent sessions were recorded in. Pass `cwd` (your current working directory) to get the directories related to it — exact, ancestor and descendant matches — plus how many conversations have no directory at all.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cwd": { "type": "string", "description": "Your current working directory. Strongly recommended: it is how this library gets scoped to the project you are in." },
                    "directory_limit": { "type": "integer", "description": "Max directories to list (default 30)." }
                }
            }
        },
        {
            "name": "list_directories",
            "description": "Every working directory that has recorded agent sessions, with conversation and message counts and activity dates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sort": { "type": "string", "enum": ["recent", "conversations", "path"], "description": "Default 'recent' (most recently active first)." },
                    "limit": { "type": "integer" },
                    "offset": { "type": "integer" }
                }
            }
        },
        {
            "name": "search",
            "description": "Full-text search across every imported conversation. Returns one snippet per conversation, best match first. Pass `cwd` to restrict to the project you are working in; omit it to search the whole library including web chats, which have no directory.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "cwd": { "type": "string", "description": "Restrict to conversations recorded in this directory, its ancestors and its descendants." },
                    "cwd_scope": { "type": "string", "enum": ["tree", "exact", "off"], "description": "Default 'tree'." },
                    "platform": { "type": "string", "enum": ["claude", "chatgpt", "deepseek", "claude-code", "codex"] },
                    "model": { "type": "string" },
                    "sender": { "type": "string", "enum": ["human", "assistant", "meta"], "description": "'human' searches only what you typed; 'assistant' only AI replies." },
                    "message_kind": { "type": "string", "enum": ["text", "tool_use", "tool_result", "thinking"] },
                    "date_from": { "type": "string", "description": "YYYY-MM-DD, inclusive." },
                    "date_to": { "type": "string", "description": "YYYY-MM-DD, inclusive." },
                    "mode": { "type": "string", "enum": ["auto", "fts", "substring"], "description": "Default 'auto': full-text, falling back to a substring scan for Chinese queries or when full-text finds nothing." },
                    "limit": { "type": "integer", "description": "Default 20, max 100." }
                }
            }
        },
        {
            "name": "list_conversations",
            "description": "Browse conversations by filter without a search term — newest first by default.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cwd": { "type": "string" },
                    "cwd_scope": { "type": "string", "enum": ["tree", "exact", "off"] },
                    "platform": { "type": "string" },
                    "model": { "type": "string" },
                    "date_from": { "type": "string" },
                    "date_to": { "type": "string" },
                    "min_messages": { "type": "integer" },
                    "max_messages": { "type": "integer" },
                    "sort": { "type": "string", "enum": ["newest", "oldest", "most_messages"] },
                    "limit": { "type": "integer", "description": "Default 20, max 100." },
                    "offset": { "type": "integer" }
                }
            }
        },
        {
            "name": "get_conversation",
            "description": "Read one conversation, paginated — including another Claude Code or Codex session, by its session uuid, which is how you look at what a different session did. Long messages are truncated per message. Agent sessions are mostly tool calls — pass kinds:[\"text\"] to read only the actual dialogue.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string", "description": "A conversation id, or a Claude Code / Codex session uuid — both resolve." },
                    "offset": { "type": "integer", "description": "Message index to start at (default 0)." },
                    "limit": { "type": "integer", "description": "Default 50, max 200." },
                    "kinds": { "type": "array", "items": { "type": "string" }, "description": "Keep only these message kinds, e.g. [\"text\"]." },
                    "max_chars_per_message": { "type": "integer", "description": "Default 2000." }
                }
            }
        },
        {
            "name": "find_session",
            "description": "Look up a Claude Code or Codex session by its session id — the uuid the CLI prints and that `--resume` takes. Use it to check what another session was about before reading it, or to get the command to resume it. Accepts the bare uuid, the stored id (cc:<uuid> / codex:<uuid>), a path to the transcript file, or a leading fragment of any of those. Returns what was captured and when, but no messages — pass the `id` it returns to get_conversation for those, or skip this tool and hand get_conversation the uuid directly.",
            "inputSchema": {
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string", "description": "e.g. \"bfa0f45e-798e-4673-b671-eed384af4c51\"." }
                }
            }
        },
        {
            "name": "get_message_context",
            "description": "Read the messages around one message id — the cheap way to follow up on a search hit without pulling the whole conversation.",
            "inputSchema": {
                "type": "object",
                "required": ["message_id"],
                "properties": {
                    "message_id": { "type": "string" },
                    "before": { "type": "integer", "description": "Default 3, max 50." },
                    "after": { "type": "integer", "description": "Default 3, max 50." },
                    "max_chars_per_message": { "type": "integer", "description": "Default 2000." }
                }
            }
        }
    ])
}

/// The tool table, split from the Tauri plumbing so it can be driven straight
/// from a `Connection` in tests.
pub fn dispatch_tool(conn: &Connection, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "overview" => tool_overview(conn, args),
        "list_directories" => tool_list_directories(conn, args),
        "search" => tool_search(conn, args),
        "list_conversations" => tool_list_conversations(conn, args),
        "get_conversation" => tool_get_conversation(conn, args),
        "find_session" => tool_find_session(conn, args),
        "get_message_context" => tool_get_message_context(conn, args),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn call_tool(app: &AppHandle, name: &str, args: &Value) -> Result<Value, String> {
    let state: State<DbState> = app.state();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    dispatch_tool(&conn, name, args)
}

/// A one-line description of what a call asked for, for the activity panel.
fn describe_call(name: &str, args: &Value) -> String {
    let mut bits = Vec::new();
    if let Some(q) = arg_str(args, "query") {
        bits.push(format!("\"{q}\""));
    }
    if let Some(id) = arg_str(args, "id").or_else(|| arg_str(args, "message_id")) {
        bits.push(id);
    }
    if let Some(cwd) = arg_str(args, "cwd") {
        bits.push(cwd);
    }
    if bits.is_empty() { name.to_string() } else { bits.join(" · ") }
}

// ─── JSON-RPC ───────────────────────────────────────────────────────────────

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Returns `None` for notifications, which get an empty 202 rather than a body.
fn handle_message(app: &AppHandle, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = msg.get("id").cloned();
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    // Notifications carry no id and must not be answered.
    let id = match id {
        Some(id) if !id.is_null() => id,
        _ => return None,
    };

    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
                requested
            } else {
                SUPPORTED_PROTOCOLS[0]
            };
            Some(rpc_ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "This is the local ChatFinder archive of the user's own AI conversations \
(Claude, ChatGPT, DeepSeek exports plus local Claude Code and Codex sessions). Call `overview` first, \
passing your current working directory as `cwd` — agent sessions are indexed by the directory they ran in, \
so that is how you find prior work on the project at hand. Note that web chats have no directory: after \
scoping by cwd, also search the whole library using the project name as a keyword. When you already know a \
Claude Code or Codex session's uuid — the one the CLI prints — you do not have to search for it: pass it to \
`get_conversation` to read that session from inside this one, or to `find_session` first to see what it is \
and how to resume it. Everything here is a snapshot taken by the last scan, never a live mirror — check each result's `snapshot.imported_at` before \
treating it as complete.",
                }),
            ))
        }
        "ping" => Some(rpc_ok(id, json!({}))),
        "tools/list" => Some(rpc_ok(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let started = std::time::Instant::now();
            let result = call_tool(app, &name, &args);
            let ms = started.elapsed().as_millis() as u64;
            let state: State<McpState> = app.state();
            state.log(&name, describe_call(&name, &args), ms, result.is_ok());

            Some(match result {
                Ok(value) => {
                    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|e| e.to_string());
                    rpc_ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": text }],
                            "structuredContent": value,
                            "isError": false,
                        }),
                    )
                }
                // A failing tool is reported inside the result, not as a
                // protocol error, so the agent can read the message and retry.
                Err(err) => rpc_ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": format!("Error: {err}") }],
                        "isError": true,
                    }),
                ),
            })
        }
        "resources/list" => Some(rpc_ok(id, json!({ "resources": [] }))),
        "prompts/list" => Some(rpc_ok(id, json!({ "prompts": [] }))),
        other => Some(rpc_error(id, -32601, &format!("method not found: {other}"))),
    }
}

// ─── HTTP ───────────────────────────────────────────────────────────────────

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_line(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<String>> {
    use std::io::BufRead;
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<HttpRequest>> {
    let request_line = match read_line(reader)? {
        Some(l) if !l.is_empty() => l,
        // A closed or idle-timed-out keep-alive connection, not an error.
        _ => return Ok(None),
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    let mut headers = Vec::new();
    while let Some(line) = read_line(reader)? {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    let len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(Some(HttpRequest { method, path, headers, body }))
}

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str, headers: &[(&str, &str)]) {
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn respond_json(stream: &mut TcpStream, status: u16, reason: &str, body: &Value) {
    respond(
        stream,
        status,
        reason,
        &body.to_string(),
        &[("Content-Type", "application/json")],
    );
}

/// Rejects anything that is not a same-machine caller. Browsers attach `Origin`
/// automatically, so a page on the open internet cannot use a victim's loopback
/// server to read their archive (DNS rebinding) even if it guessed the port.
/// Native MCP clients send no Origin at all, which is why absence is allowed.
fn origin_allowed(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(o) => {
            let o = o.trim().to_ascii_lowercase();
            o == "null"
                || o.starts_with("http://localhost")
                || o.starts_with("http://127.0.0.1")
                || o.starts_with("https://localhost")
                || o.starts_with("https://127.0.0.1")
        }
    }
}

/// What the policy layer decided to send back. Separated from the socket so
/// auth, origin and routing can be tested without any I/O.
struct HttpResponse {
    status: u16,
    reason: &'static str,
    body: String,
    headers: Vec<(&'static str, String)>,
}

impl HttpResponse {
    fn json(status: u16, reason: &'static str, body: Value) -> Self {
        HttpResponse {
            status,
            reason,
            body: body.to_string(),
            headers: vec![("Content-Type", "application/json".into())],
        }
    }

    fn text(status: u16, reason: &'static str, body: String) -> Self {
        HttpResponse {
            status,
            reason,
            body,
            headers: vec![("Content-Type", "text/plain; charset=utf-8".into())],
        }
    }

    fn empty(status: u16, reason: &'static str) -> Self {
        HttpResponse { status, reason, body: String::new(), headers: Vec::new() }
    }
}

/// Decides what a request gets, with no sockets and no Tauri in sight.
///
/// `rpc` handles a JSON-RPC message, returning `None` for a notification.
/// `take_enrollment` yields the token at most once per opened window.
fn route(
    request: &HttpRequest,
    token: &str,
    take_enrollment: &dyn Fn() -> Option<String>,
    rpc: &dyn Fn(&Value) -> Option<Value>,
) -> HttpResponse {
    if !origin_allowed(request.header("origin")) {
        return HttpResponse::json(403, "Forbidden", json!({ "error": "origin not allowed" }));
    }

    let path = request.path.split('?').next().unwrap_or("/");

    // Deliberately ahead of the auth check — collecting the token is the one
    // thing a client cannot already have the token for. The Origin check above
    // still applies, and matters more here than anywhere else.
    if path == "/enroll" {
        return match take_enrollment() {
            Some(token) => HttpResponse::text(200, "OK", token),
            None => HttpResponse::json(
                403,
                "Forbidden",
                json!({ "error": "no enrollment window is open — open one in ChatFinder's MCP settings" }),
            ),
        };
    }

    let authorized = request
        .header("authorization")
        .map(|h| h.trim() == format!("Bearer {token}"))
        .unwrap_or(false);
    if !authorized {
        let mut response = HttpResponse::json(
            401,
            "Unauthorized",
            json!({ "error": "missing or invalid bearer token" }),
        );
        response.headers.push(("WWW-Authenticate", "Bearer".into()));
        return response;
    }

    if path != "/mcp" && path != "/" {
        return HttpResponse::json(404, "Not Found", json!({ "error": "not found" }));
    }

    match request.method.as_str() {
        // No server-initiated stream is offered; the spec's prescribed answer
        // for that is 405, and clients fall back to plain POST.
        "GET" => HttpResponse::json(
            405,
            "Method Not Allowed",
            json!({ "error": "SSE stream not supported; POST JSON-RPC to this endpoint" }),
        ),
        "DELETE" => HttpResponse::empty(200, "OK"),
        "POST" => {
            let Ok(payload) = serde_json::from_slice::<Value>(&request.body) else {
                return HttpResponse::json(400, "Bad Request", rpc_error(Value::Null, -32700, "parse error"));
            };
            let response = match &payload {
                Value::Array(items) => {
                    let replies: Vec<Value> = items.iter().filter_map(rpc).collect();
                    if replies.is_empty() { None } else { Some(Value::Array(replies)) }
                }
                _ => rpc(&payload),
            };
            match response {
                Some(body) => HttpResponse::json(200, "OK", body),
                None => HttpResponse::empty(202, "Accepted"),
            }
        }
        _ => HttpResponse::json(405, "Method Not Allowed", json!({ "error": "unsupported method" })),
    }
}

fn handle_connection(stream: TcpStream, app: AppHandle, token: String) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(120)));
    let mut write_half = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);

    loop {
        let request = match read_request(&mut reader) {
            Ok(Some(r)) => r,
            _ => return,
        };
        let response = route(
            &request,
            &token,
            &|| take_enrollment_token(&app),
            &|msg| handle_message(&app, msg),
        );
        let headers: Vec<(&str, &str)> = response
            .headers
            .iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        respond(&mut write_half, response.status, response.reason, &response.body, &headers);

        // Anything the caller was not allowed to do ends the connection rather
        // than leaving it open to retry on.
        if response.status == 401 || response.status == 403 {
            return;
        }
    }
}

// ─── lifecycle ──────────────────────────────────────────────────────────────

fn read_config(app: &AppHandle) -> Result<(bool, u16, String), String> {
    let state: State<DbState> = app.state();
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let enabled = get_setting(&conn, KEY_ENABLED).as_deref() == Some("1");
    let port = get_setting(&conn, KEY_PORT)
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let token = match get_setting(&conn, KEY_TOKEN) {
        Some(t) if !t.is_empty() => t,
        _ => {
            let t = new_token();
            put_setting(&conn, KEY_TOKEN, &t)?;
            t
        }
    };
    Ok((enabled, port, token))
}

/// Binds loopback, walking forward a few ports if the preferred one is taken —
/// a stale process or another copy of the app shouldn't leave the feature dead
/// with nothing but "address in use".
fn bind_loopback(preferred: u16) -> Result<(TcpListener, u16), String> {
    let mut last = String::new();
    for port in preferred..preferred.saturating_add(10) {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok((l, port)),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("could not bind a port from {preferred} onwards: {last}"))
}

pub fn stop_server(app: &AppHandle) {
    let state = app.state::<McpState>();
    let previous = state.running.lock().ok().and_then(|mut g| g.take());
    if let Some(running) = previous {
        running.stop.store(true, Ordering::Relaxed);
    }
}

pub fn start_server(app: &AppHandle) -> Result<u16, String> {
    stop_server(app);
    let (_, preferred, token) = read_config(app)?;
    let (listener, port) = bind_loopback(preferred)?;
    // Non-blocking accept + a short sleep is what lets the toggle stop the
    // server promptly; a blocking accept() would sit until the next request.
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_app = app.clone();
    std::thread::Builder::new()
        .name("chatfinder-mcp".into())
        .spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let app = thread_app.clone();
                        let token = token.clone();
                        std::thread::spawn(move || handle_connection(stream, app, token));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(150));
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(150)),
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let state = app.state::<McpState>();
    if let Ok(mut guard) = state.running.lock() {
        *guard = Some(Running { port, stop });
    }
    if let Ok(mut err) = state.last_error.lock() {
        *err = None;
    }
    drop(state);
    Ok(port)
}

/// Called once at startup so the server comes back on its own if the user left
/// it on. A failure here is recorded and surfaced in settings rather than
/// panicking the app — the archive still works without MCP.
pub fn start_if_enabled(app: &AppHandle) {
    let enabled = read_config(app).map(|(e, _, _)| e).unwrap_or(false);
    if !enabled {
        return;
    }
    if let Err(e) = start_server(app) {
        let state = app.state::<McpState>();
        let slot = state.last_error.lock();
        if let Ok(mut err) = slot {
            *err = Some(e);
        }
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

/// One client's registration snippet. The panel renders the label and the
/// where-to-put-it hint from `client`, so this stays language-neutral.
#[derive(Serialize)]
pub struct SetupSnippet {
    /// "claude_code" | "codex" | "json"
    pub client: String,
    /// Syntax to highlight and how to present it: "shell" | "toml" | "json".
    pub kind: String,
    pub content: String,
    /// The same registration, but fetching the token from `/enroll` at run time
    /// instead of embedding it. Safe to paste into an agent conversation —
    /// nothing but the variable name `$TOKEN` survives in the transcript.
    /// `None` for clients configured through a GUI, where a command cannot help.
    pub enroll: Option<String>,
}

/// Registration for every client shape this server is known to work with.
///
/// Claude Code takes a one-liner. Codex accepts a streamable-HTTP server too,
/// but `codex mcp add` can only reference a bearer token through an environment
/// variable, so a config block with an inline header is one paste instead of
/// two steps. Everything else follows the conventional `mcpServers` JSON.
fn setup_snippets(url: &str, token: &str) -> Vec<SetupSnippet> {
    setup_snippets_for(url, token, cfg!(target_os = "windows"))
}

/// `powershell`: spell the one-liners for PowerShell rather than a POSIX shell.
/// Split out from `setup_snippets` so both dialects stay testable from any host.
fn setup_snippets_for(url: &str, token: &str, powershell: bool) -> Vec<SetupSnippet> {
    let enroll_url = url.trim_end_matches("/mcp").to_string() + "/enroll";
    let claude_add = |tok: &str| {
        format!(
            "claude mcp add --scope user --transport http {SERVER_NAME} {url} --header \"Authorization: Bearer {tok}\""
        )
    };
    let codex_toml = |tok: &str| {
        format!(
            "[mcp_servers.{SERVER_NAME}]\nurl = \"{url}\"\n\n[mcp_servers.{SERVER_NAME}.http_headers]\nAuthorization = \"Bearer {tok}\""
        )
    };

    let (shell_kind, claude_enroll, codex_enroll) = if powershell {
        // `.Trim()` because the endpoint answers with a bare text body, and a
        // trailing newline would travel into the Authorization header.
        let fetch = format!("$TOKEN = (Invoke-RestMethod -Uri {enroll_url}).Trim()");
        (
            "powershell",
            format!("{fetch}; {}", claude_add("$TOKEN")),
            // A here-string keeps the TOML literal: PowerShell expands $TOKEN
            // inside it but leaves the quotes and newlines alone.
            format!(
                "{fetch}\nAdd-Content -Path \"$env:USERPROFILE\\.codex\\config.toml\" -Value @\"\n\n{}\n\"@",
                codex_toml("$TOKEN")
            ),
        )
    } else {
        let fetch = format!("TOKEN=$(curl -sf {enroll_url})");
        (
            "shell",
            format!("{fetch} && {}", claude_add("$TOKEN")),
            // printf keeps the token in an argument rather than in the format
            // string, so a stray % in it cannot be interpreted.
            format!(
                "{fetch} && printf '\\n[mcp_servers.{SERVER_NAME}]\\nurl = \"{url}\"\\n\\n[mcp_servers.{SERVER_NAME}.http_headers]\\nAuthorization = \"Bearer %s\"\\n' \"$TOKEN\" >> ~/.codex/config.toml"
            ),
        )
    };

    vec![
        SetupSnippet {
            client: "claude_code".into(),
            kind: shell_kind.into(),
            content: claude_add(token),
            enroll: Some(claude_enroll),
        },
        SetupSnippet {
            client: "codex".into(),
            kind: "toml".into(),
            content: codex_toml(token),
            // `codex mcp add` can only point at an environment variable name,
            // never an inline header, so the config block is appended directly.
            enroll: Some(codex_enroll),
        },
        SetupSnippet {
            client: "json".into(),
            kind: "json".into(),
            content: format!(
                "{{\n  \"mcpServers\": {{\n    \"{SERVER_NAME}\": {{\n      \"type\": \"http\",\n      \"url\": \"{url}\",\n      \"headers\": {{\n        \"Authorization\": \"Bearer {token}\"\n      }}\n    }}\n  }}\n}}"
            ),
            enroll: None,
        },
    ]
}

#[derive(Serialize)]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    /// The port actually bound, which can differ from the configured one when
    /// that was taken.
    pub port: u16,
    pub configured_port: u16,
    pub token: String,
    pub url: String,
    /// Ready-to-paste client registrations, the thing users actually need.
    pub setup: Vec<SetupSnippet>,
    /// Seconds left on the self-service enrollment window; 0 when closed.
    pub enrollment_seconds_left: u64,
    pub last_error: Option<String>,
}

#[tauri::command]
pub fn mcp_status(app: AppHandle) -> Result<McpStatus, String> {
    let (enabled, configured_port, token) = read_config(&app)?;
    let state: State<McpState> = app.state();
    let running_port = state.running.lock().ok().and_then(|g| g.as_ref().map(|r| r.port));
    let port = running_port.unwrap_or(configured_port);
    let url = format!("http://127.0.0.1:{port}/mcp");
    Ok(McpStatus {
        enabled,
        running: running_port.is_some(),
        port,
        configured_port,
        setup: setup_snippets(&url, &token),
        enrollment_seconds_left: enrollment_seconds_left(&app),
        token,
        url,
        last_error: state.last_error.lock().ok().and_then(|e| e.clone()),
    })
}

#[tauri::command]
pub fn mcp_set_enabled(app: AppHandle, enabled: bool) -> Result<McpStatus, String> {
    {
        let state: State<DbState> = app.state();
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        put_setting(&conn, KEY_ENABLED, if enabled { "1" } else { "0" })?;
    }
    if enabled {
        start_server(&app)?;
    } else {
        stop_server(&app);
    }
    mcp_status(app)
}

#[tauri::command]
pub fn mcp_set_port(app: AppHandle, port: u16) -> Result<McpStatus, String> {
    if port < 1024 {
        return Err("pick a port of 1024 or above".into());
    }
    let was_running = {
        let state: State<McpState> = app.state();
        let running = state.running.lock().map_err(|e| e.to_string())?.is_some();
        running
    };
    {
        let state: State<DbState> = app.state();
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        put_setting(&conn, KEY_PORT, &port.to_string())?;
    }
    if was_running {
        start_server(&app)?;
    }
    mcp_status(app)
}

/// Invalidates the old token immediately by restarting the listener with the
/// new one — the running server captured the previous value.
#[tauri::command]
pub fn mcp_regenerate_token(app: AppHandle) -> Result<McpStatus, String> {
    {
        let state: State<DbState> = app.state();
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        put_setting(&conn, KEY_TOKEN, &new_token())?;
    }
    let was_running = {
        let state: State<McpState> = app.state();
        let running = state.running.lock().map_err(|e| e.to_string())?.is_some();
        running
    };
    if was_running {
        start_server(&app)?;
    }
    mcp_status(app)
}

/// Opens the self-service window. Requires a running server — otherwise the
/// command the user is about to hand an agent would just fail to connect.
#[tauri::command]
pub fn mcp_open_enrollment(app: AppHandle) -> Result<McpStatus, String> {
    let (_, _, token) = read_config(&app)?;
    {
        let state = app.state::<McpState>();
        let running = state.running.lock().map_err(|e| e.to_string())?.is_some();
        if !running {
            return Err("start the MCP server first".into());
        }
        let mut guard = state.enrollment.lock().map_err(|e| e.to_string())?;
        *guard = Some(Enrollment {
            token,
            expires_at: std::time::Instant::now() + ENROLLMENT_TTL,
        });
    }
    mcp_status(app)
}

#[tauri::command]
pub fn mcp_cancel_enrollment(app: AppHandle) -> Result<McpStatus, String> {
    {
        let state = app.state::<McpState>();
        let mut guard = state.enrollment.lock().map_err(|e| e.to_string())?;
        *guard = None;
    }
    mcp_status(app)
}

#[tauri::command]
pub fn mcp_activity(app: AppHandle) -> Result<Vec<ActivityEntry>, String> {
    let state: State<McpState> = app.state();
    let log = state.activity.lock().map_err(|e| e.to_string())?;
    Ok(log.iter().rev().cloned().collect())
}

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_session_recorded_at_the_project_root_from_a_subdirectory() {
        // The case that motivated tree matching: sessions are recorded at the
        // project root, but an agent is often started deeper in the tree.
        assert_eq!(
            relation(
                "/Users/dev/PycharmProjects/proj/app/src-tauri/src",
                "/Users/dev/PycharmProjects/proj"
            ),
            Some(Rel::Ancestor)
        );
        assert_eq!(
            relation("/Users/dev/PycharmProjects/proj", "/Users/dev/PycharmProjects/proj/app"),
            Some(Rel::Descendant)
        );
        assert_eq!(
            relation("/Users/dev/proj/", "/Users/dev/proj"),
            Some(Rel::Exact)
        );
    }

    #[test]
    fn does_not_treat_the_home_directory_as_a_related_project() {
        // /Users/dev is an ancestor of everything the person owns, so allowing
        // it would make cwd scoping match the entire library.
        assert_eq!(relation("/Users/dev/PycharmProjects/proj", "/Users/dev"), None);
        assert_eq!(relation("/Users/dev/PycharmProjects/proj", "/Users"), None);
        // A directory that deep is still a real project, in both directions.
        assert_eq!(
            relation("/Users/dev/code/proj", "/Users/dev/code"),
            Some(Rel::Ancestor)
        );
    }

    #[test]
    fn unrelated_and_sibling_directories_do_not_match() {
        assert_eq!(relation("/Users/dev/proj-a", "/Users/dev/proj-b"), None);
        // A shared name prefix is not a shared path component.
        assert_eq!(relation("/Users/dev/code/proj", "/Users/dev/code/proj2"), None);
    }

    #[test]
    fn windows_paths_match_the_same_way_posix_ones_do() {
        // Recorded cwds come from the agent transcript verbatim, so on Windows
        // every path in play is backslash-separated and drive-rooted.
        assert_eq!(
            relation(r"C:\Users\dev\Code\proj\app\src", r"C:\Users\dev\Code\proj"),
            Some(Rel::Ancestor)
        );
        assert_eq!(
            relation(r"C:\Users\dev\Code\proj", r"C:\Users\dev\Code\proj\app"),
            Some(Rel::Descendant)
        );
        assert_eq!(
            relation(r"C:\Users\Dev\Code\Proj", r"c:\users\dev\code\proj"),
            Some(Rel::Exact)
        );
        assert_eq!(relation(r"C:\Users\dev\proj-a", r"C:\Users\dev\proj-b"), None);
        // A trailing separator is noise, in either spelling.
        assert_eq!(relation(r"C:\Users\dev\proj\", r"C:\Users\dev\proj"), Some(Rel::Exact));
    }

    #[test]
    fn the_drive_letter_does_not_buy_a_level_of_depth() {
        // C:\Users\dev is the home directory, just like /Users/dev — counting
        // the drive as a component would let it through as a "project".
        assert_eq!(relation(r"C:\Users\dev\Code\proj", r"C:\Users\dev"), None);
        assert_eq!(relation(r"C:\Users\dev\Code\proj", r"C:\Users"), None);
        assert_eq!(
            relation(r"C:\Users\dev\Code\proj", r"C:\Users\dev\Code"),
            Some(Rel::Ancestor)
        );
    }

    #[test]
    fn the_extended_length_prefix_is_stripped_before_matching() {
        // Windows `canonicalize` returns `\\?\C:\...`, but a session transcript
        // records the plain `C:\...` — left alone, the query side would never
        // match anything the scanner stored.
        assert_eq!(normalize_path(r"\\?\C:\Users\dev\proj"), r"C:\Users\dev\proj");
        assert_eq!(normalize_path(r"\\?\UNC\server\share\proj"), r"\\server\share\proj");
        assert_eq!(
            relation(&normalize_path(r"\\?\C:\Users\dev\proj"), r"C:\Users\dev\proj"),
            Some(Rel::Exact)
        );
        // A trailing backslash is trimmed the way a trailing slash always was.
        assert_eq!(normalize_path(r"C:\Users\dev\proj\"), r"C:\Users\dev\proj");
    }

    #[test]
    fn different_drives_are_never_related() {
        assert_eq!(relation(r"C:\Users\dev\Code\proj", r"D:\Users\dev\Code\proj"), None);
    }

    #[test]
    fn directory_matching_ignores_case_like_the_filesystem_does() {
        assert_eq!(
            relation("/Users/Dev/Code/Proj", "/users/dev/code/proj"),
            Some(Rel::Exact)
        );
    }

    #[test]
    fn snippet_marks_the_match_and_trims_both_sides() {
        let text = "a".repeat(200) + "needle" + &"b".repeat(200);
        let s = make_snippet(&text, "needle", 10);
        assert!(s.contains("【needle】"), "got: {s}");
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.chars().count() < 50);
    }

    #[test]
    fn truncation_reports_how_much_it_dropped() {
        let (out, cut) = truncate(&"x".repeat(100), 10);
        assert!(cut);
        assert!(out.contains("90 more characters"));
        let (out, cut) = truncate("short", 10);
        assert!(!cut);
        assert_eq!(out, "short");
    }

    #[test]
    fn chinese_queries_are_routed_around_the_fts_tokenizer() {
        assert!(is_cjk_query("重构方案"));
        assert!(is_cjk_query("fix 数据库"));
        assert!(!is_cjk_query("database migration"));
    }

    #[test]
    fn a_match_inside_a_base64_blob_is_recognised_as_noise() {
        // The shape actually seen in the archive: FTS5 split a base64 image into
        // fragments and one of them matched a short query.
        let noisy = "//LUny8vLS448/ritXruju3bv6/ffftWbNGi1ZssRpEJTVXL9+XefPn5dk//【mcPXtWe】/bs0aJFi+Tv76/p06frySefzKhmOvXjjz9";
        assert!(match_sits_in_an_encoded_blob(noisy));
    }

    #[test]
    fn ordinary_prose_is_never_mistaken_for_an_encoded_blob() {
        assert!(!match_sits_in_an_encoded_blob("how do I run the database 【migration】 for this project"));
        // CJK has no spaces but is outside the character class, so it is safe.
        assert!(!match_sits_in_an_encoded_blob("先做【数据库迁移】然后重新导入所有的历史对话记录再验证一次结果"));
        // A long token elsewhere in the message must not disqualify a real match.
        assert!(!match_sits_in_an_encoded_blob(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9eyJzdWIiOiIxMjM0NTY3ODkwIn0 and then 【cargo】 test"
        ));
    }

    #[test]
    fn the_enrollment_command_never_contains_the_token() {
        // The whole reason enrollment exists: this string gets pasted into an
        // agent conversation, which is written to a transcript that this app
        // itself imports and indexes. A token here would end up searchable
        // inside the archive it protects.
        let token = "s3cr3ttokenvalue0000000000000000";
        let both = [true, false].into_iter().flat_map(|ps| {
            setup_snippets_for("http://127.0.0.1:8722/mcp", token, ps)
        });
        for snippet in both {
            if let Some(enroll) = &snippet.enroll {
                assert!(
                    !enroll.contains(token),
                    "{} enrollment command leaks the token: {enroll}",
                    snippet.client
                );
                assert!(enroll.contains("/enroll"), "{} must fetch it instead", snippet.client);
                assert!(enroll.contains("$TOKEN"), "{} must pass it by variable", snippet.client);
            }
            // The manual snippet is the opposite case — it is meant to carry it.
            assert!(snippet.content.contains(token), "{} manual snippet", snippet.client);
        }
    }

    #[test]
    fn claude_code_is_registered_for_every_project_not_just_one() {
        // `claude mcp add` defaults to `local` scope, which silently binds the
        // server to whatever directory the command ran in — the archive then
        // vanishes in every other project. `user` is the scope that matches
        // what this server is: one personal library, available everywhere.
        for snippet in setup_snippets("http://127.0.0.1:8722/mcp", "tok") {
            if snippet.client != "claude_code" {
                continue;
            }
            assert!(snippet.content.contains("--scope user"), "manual: {}", snippet.content);
            let enroll = snippet.enroll.as_ref().unwrap();
            assert!(enroll.contains("--scope user"), "enroll: {enroll}");
        }
    }

    #[test]
    fn gui_configured_clients_get_no_enrollment_command() {
        // A shell one-liner cannot fill in someone's config UI, so offering one
        // would only mislead.
        let snippets = setup_snippets("http://127.0.0.1:8722/mcp", "tok");
        let json = snippets.iter().find(|s| s.client == "json").unwrap();
        assert!(json.enroll.is_none());
        assert_eq!(snippets.iter().filter(|s| s.enroll.is_some()).count(), 2);
    }

    #[test]
    fn windows_gets_powershell_one_liners_not_posix_ones() {
        // A POSIX one-liner pasted into PowerShell fails in ways that look like
        // the server is broken: `TOKEN=$(...)` is a parse error and `>>` writes
        // UTF-16. Both clients have to be spelled for the host's shell.
        let ps = setup_snippets_for("http://127.0.0.1:8722/mcp", "tok", true);
        let claude = ps.iter().find(|s| s.client == "claude_code").unwrap();
        assert_eq!(claude.kind, "powershell");
        let enroll = claude.enroll.as_ref().unwrap();
        assert!(enroll.contains("Invoke-RestMethod"), "got: {enroll}");
        assert!(!enroll.contains("curl -sf"), "got: {enroll}");

        let codex = ps.iter().find(|s| s.client == "codex").unwrap();
        let enroll = codex.enroll.as_ref().unwrap();
        // `~` is not expanded by PowerShell cmdlets the way a shell expands it.
        assert!(enroll.contains("$env:USERPROFILE"), "got: {enroll}");
        assert!(!enroll.contains("~/.codex"), "got: {enroll}");

        // The POSIX variant must stay POSIX.
        let posix = setup_snippets_for("http://127.0.0.1:8722/mcp", "tok", false);
        let claude = posix.iter().find(|s| s.client == "claude_code").unwrap();
        assert_eq!(claude.kind, "shell");
        assert!(claude.enroll.as_ref().unwrap().contains("curl -sf"));
    }

    #[test]
    fn the_enroll_url_sits_beside_the_mcp_endpoint() {
        let snippets = setup_snippets("http://127.0.0.1:9999/mcp", "tok");
        let enroll = snippets[0].enroll.as_ref().unwrap();
        assert!(enroll.contains("http://127.0.0.1:9999/enroll"), "got: {enroll}");
    }

    fn slot_with(expires_in: std::time::Duration) -> Mutex<Option<Enrollment>> {
        Mutex::new(Some(Enrollment {
            token: "the-real-token".into(),
            expires_at: std::time::Instant::now() + expires_in,
        }))
    }

    #[test]
    fn an_enrollment_window_can_only_be_used_once() {
        // The endpoint is unauthenticated by necessity, so a window that stayed
        // open after being used would be a standing invitation to any local
        // process.
        let slot = slot_with(std::time::Duration::from_secs(300));
        assert_eq!(take_from_slot(&slot).as_deref(), Some("the-real-token"));
        assert_eq!(take_from_slot(&slot), None, "a second fetch must find nothing");
    }

    #[test]
    fn an_expired_window_hands_out_nothing_and_clears_itself() {
        let slot = Mutex::new(Some(Enrollment {
            token: "the-real-token".into(),
            // Already past.
            expires_at: std::time::Instant::now() - std::time::Duration::from_secs(1),
        }));
        assert_eq!(take_from_slot(&slot), None);
        assert!(slot.lock().unwrap().is_none(), "the stale window should be dropped");
    }

    #[test]
    fn a_closed_window_hands_out_nothing() {
        let slot: Mutex<Option<Enrollment>> = Mutex::new(None);
        assert_eq!(take_from_slot(&slot), None);
    }

    fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            path: path.into(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn no_enrollment() -> Option<String> {
        None
    }

    fn no_rpc(_: &Value) -> Option<Value> {
        Some(json!({ "ok": true }))
    }

    const TOKEN: &str = "correct-token";
    const AUTH: (&str, &str) = ("Authorization", "Bearer correct-token");

    #[test]
    fn the_enroll_endpoint_hands_the_token_over_without_a_token() {
        // The bootstrapping case: a client that has nothing yet must be able to
        // collect the credential, or self-service enrollment is impossible.
        let slot = slot_with(std::time::Duration::from_secs(300));
        let response = route(
            &request("GET", "/enroll", &[], ""),
            TOKEN,
            &|| take_from_slot(&slot),
            &no_rpc,
        );
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "the-real-token");
        // Body is bare text so `$(curl -s ...)` yields the token directly.
        assert!(!response.body.contains('{'));

        // And the window is now spent.
        let again = route(&request("GET", "/enroll", &[], ""), TOKEN, &|| take_from_slot(&slot), &no_rpc);
        assert_eq!(again.status, 403);
    }

    #[test]
    fn enrollment_is_still_refused_to_a_foreign_origin() {
        // An unauthenticated endpoint is exactly where a rebinding attack would
        // aim, so the Origin check has to come first.
        let slot = slot_with(std::time::Duration::from_secs(300));
        let response = route(
            &request("GET", "/enroll", &[("Origin", "https://evil.example.com")], ""),
            TOKEN,
            &|| take_from_slot(&slot),
            &no_rpc,
        );
        assert_eq!(response.status, 403);
        assert!(response.body.contains("origin"));
        // Crucially, the window must survive a rejected attempt.
        assert!(slot.lock().unwrap().is_some(), "a blocked request must not burn the window");
    }

    #[test]
    fn the_mcp_endpoint_still_demands_the_token() {
        let deny = |req: HttpRequest| route(&req, TOKEN, &no_enrollment, &no_rpc);
        assert_eq!(deny(request("POST", "/mcp", &[], "{}")).status, 401);
        assert_eq!(deny(request("POST", "/mcp", &[("Authorization", "Bearer wrong")], "{}")).status, 401);
        assert_eq!(deny(request("POST", "/mcp", &[AUTH], "{}")).status, 200);
    }

    #[test]
    fn protocol_level_routing_matches_what_clients_expect() {
        let call = |req: HttpRequest| route(&req, TOKEN, &no_enrollment, &no_rpc);
        // No SSE stream offered — 405 is the spec's prescribed answer.
        assert_eq!(call(request("GET", "/mcp", &[AUTH], "")).status, 405);
        assert_eq!(call(request("DELETE", "/mcp", &[AUTH], "")).status, 200);
        assert_eq!(call(request("POST", "/nope", &[AUTH], "{}")).status, 404);
        assert_eq!(call(request("POST", "/mcp", &[AUTH], "not json")).status, 400);
        // A notification carries no id and must get an empty 202, not a body.
        let notify = route(
            &request("POST", "/mcp", &[AUTH], r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            TOKEN,
            &no_enrollment,
            &|msg| handle_message_for_test(msg),
        );
        assert_eq!(notify.status, 202);
        assert!(notify.body.is_empty());
    }

    /// Mirrors `handle_message`'s notification rule without needing an AppHandle.
    fn handle_message_for_test(msg: &Value) -> Option<Value> {
        match msg.get("id") {
            Some(id) if !id.is_null() => Some(rpc_ok(id.clone(), json!({}))),
            _ => None,
        }
    }

    #[test]
    fn only_loopback_origins_are_accepted() {
        assert!(origin_allowed(None)); // native MCP clients send no Origin
        assert!(origin_allowed(Some("http://localhost:3000")));
        assert!(origin_allowed(Some("http://127.0.0.1:5173")));
        assert!(!origin_allowed(Some("https://evil.example.com")));
    }
}
