//! Keeps the archive current by rescanning local agent sessions on a timer.
//!
//! Imports are snapshots: a Claude Code or Codex session only reaches the
//! library when a scan picks it up, so without this everything the MCP server
//! serves is as old as the last time the user pressed the button. An agent
//! asking about work from an hour ago would silently get nothing.
//!
//! A full rescan of ~300 MB of transcripts parses in about 1.4s cold and 0.5s
//! once every conversation is already stored and dedup short-circuits, so even
//! the shortest interval here is cheap. That only holds because scanning is
//! genuinely idempotent — see the Codex `session_meta` note in agent_scan.rs.

use crate::db::DbState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

const KEY_ENABLED: &str = "autosync_enabled";
const KEY_INTERVAL: &str = "autosync_interval_minutes";
const KEY_LAST_RUN: &str = "autosync_last_run";
const KEY_LAST_RESULT: &str = "autosync_last_result";

/// Offered in the UI. Anything shorter buys nothing: a session is written to
/// disk continuously, so syncing more often just re-reads the same bytes.
pub const INTERVAL_CHOICES: [i64; 5] = [15, 30, 60, 360, 1440];
const DEFAULT_INTERVAL: i64 = 60;

/// How long after launch the first sync runs. Long enough to stay out of the
/// way of the window opening and the first queries.
const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(20);

/// How often the loop wakes to look at the clock. Short enough that toggling
/// the feature off takes effect promptly.
const TICK: std::time::Duration = std::time::Duration::from_secs(10);

fn get_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .ok()
}

fn put_setting(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// The manual folder picker stores one override per tool for sandboxed builds
/// that cannot reach `~/.claude` by default. A timed scan has no UI to fall
/// back on, so it reuses whatever the user already granted.
fn dir_overrides(conn: &rusqlite::Connection) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT key, value FROM settings WHERE key LIKE 'agent_dir:%'")
    else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
        for row in rows.flatten() {
            if let Some(tool) = row.0.strip_prefix("agent_dir:") {
                if !row.1.is_empty() {
                    out.insert(tool.to_string(), row.1);
                }
            }
        }
    }
    out
}

#[derive(Default)]
pub struct AutoSyncState {
    running: Mutex<Option<Arc<AtomicBool>>>,
    /// Set while a scan is in flight, so a manual import and a timed one cannot
    /// queue up behind each other on the database lock.
    in_flight: Arc<AtomicBool>,
}

#[derive(Serialize, Clone)]
pub struct AutoSyncStatus {
    pub enabled: bool,
    pub interval_minutes: i64,
    pub interval_choices: Vec<i64>,
    pub last_run: Option<String>,
    /// Human-readable outcome of the last run, or the error it failed with.
    pub last_result: Option<String>,
    pub in_flight: bool,
}

fn read_config(conn: &rusqlite::Connection) -> (bool, i64) {
    let enabled = get_setting(conn, KEY_ENABLED).as_deref() == Some("1");
    let interval = get_setting(conn, KEY_INTERVAL)
        .and_then(|v| v.parse().ok())
        .filter(|m| INTERVAL_CHOICES.contains(m))
        .unwrap_or(DEFAULT_INTERVAL);
    (enabled, interval)
}

/// Runs one scan. Returns the summary line recorded as `last_result`.
fn sync_once(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AutoSyncState>();
    // `swap` rather than load-then-store: two threads must not both decide the
    // coast is clear.
    if state.in_flight.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    let in_flight = state.in_flight.clone();
    let _guard = scopeguard(move || in_flight.store(false, Ordering::SeqCst));

    let batch_id = uuid::Uuid::new_v4().to_string();
    let db: State<DbState> = app.state();
    let mut conn = db.0.lock().map_err(|e| e.to_string())?;
    let overrides = dir_overrides(&conn);
    let summary = crate::agent_scan::import_agent_sessions(&mut conn, &[], &overrides, &batch_id, None)?;

    let line = format!(
        "added {}, updated {}, skipped {}",
        summary.added, summary.updated, summary.skipped
    );
    let now = chrono::Utc::now().to_rfc3339();
    put_setting(&conn, KEY_LAST_RUN, &now)?;
    put_setting(&conn, KEY_LAST_RESULT, &line)?;
    drop(conn);

    // Only wake the UI when something actually landed; a no-op scan should not
    // make the conversation list flicker every interval.
    if summary.added > 0 || summary.updated > 0 {
        let _ = app.emit("autosync:imported", line.clone());
    }
    Ok(line)
}

/// Minimal RAII guard — `scopeguard` the crate is not a dependency and this is
/// the only place that needs one.
fn scopeguard<F: FnOnce()>(f: F) -> impl Drop {
    struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    Guard(Some(f))
}

pub fn stop(app: &AppHandle) {
    let state = app.state::<AutoSyncState>();
    let previous = state.running.lock().ok().and_then(|mut g| g.take());
    if let Some(flag) = previous {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn start(app: &AppHandle) {
    stop(app);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let thread_stop = stop_flag.clone();
    let thread_app = app.clone();

    std::thread::Builder::new()
        .name("chatfinder-autosync".into())
        .spawn(move || {
            let mut next_due = std::time::Instant::now() + STARTUP_DELAY;
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(TICK);
                if thread_stop.load(Ordering::Relaxed) {
                    return;
                }
                if std::time::Instant::now() < next_due {
                    continue;
                }
                // Re-read each cycle so a changed interval takes effect without
                // restarting the thread.
                let interval = {
                    let db: State<DbState> = thread_app.state();
                    let locked = db.0.lock();
                    match locked {
                        Ok(conn) => {
                            let (enabled, interval) = read_config(&conn);
                            if !enabled {
                                return;
                            }
                            interval
                        }
                        Err(_) => DEFAULT_INTERVAL,
                    }
                };
                if let Err(e) = sync_once(&thread_app) {
                    let db: State<DbState> = thread_app.state();
                    let locked = db.0.lock();
                    if let Ok(conn) = locked {
                        let _ = put_setting(&conn, KEY_LAST_RESULT, &format!("failed: {e}"));
                        let _ = put_setting(&conn, KEY_LAST_RUN, &chrono::Utc::now().to_rfc3339());
                    }
                }
                next_due = std::time::Instant::now()
                    + std::time::Duration::from_secs((interval * 60) as u64);
            }
        })
        .ok();

    let state = app.state::<AutoSyncState>();
    let slot = state.running.lock();
    if let Ok(mut guard) = slot {
        *guard = Some(stop_flag);
    }
}

pub fn start_if_enabled(app: &AppHandle) {
    let enabled = {
        let db: State<DbState> = app.state();
        let locked = db.0.lock();
        match locked {
            Ok(conn) => read_config(&conn).0,
            Err(_) => false,
        }
    };
    if enabled {
        start(app);
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn autosync_status(app: AppHandle) -> Result<AutoSyncStatus, String> {
    let db: State<DbState> = app.state();
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (enabled, interval_minutes) = read_config(&conn);
    let status = AutoSyncStatus {
        enabled,
        interval_minutes,
        interval_choices: INTERVAL_CHOICES.to_vec(),
        last_run: get_setting(&conn, KEY_LAST_RUN),
        last_result: get_setting(&conn, KEY_LAST_RESULT),
        in_flight: app.state::<AutoSyncState>().in_flight.load(Ordering::SeqCst),
    };
    Ok(status)
}

#[tauri::command]
pub fn autosync_set_enabled(app: AppHandle, enabled: bool) -> Result<AutoSyncStatus, String> {
    {
        let db: State<DbState> = app.state();
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        put_setting(&conn, KEY_ENABLED, if enabled { "1" } else { "0" })?;
    }
    if enabled {
        start(&app);
    } else {
        stop(&app);
    }
    autosync_status(app)
}

#[tauri::command]
pub fn autosync_set_interval(app: AppHandle, minutes: i64) -> Result<AutoSyncStatus, String> {
    if !INTERVAL_CHOICES.contains(&minutes) {
        return Err(format!("unsupported interval: {minutes}"));
    }
    let was_enabled = {
        let db: State<DbState> = app.state();
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        put_setting(&conn, KEY_INTERVAL, &minutes.to_string())?;
        read_config(&conn).0
    };
    // Restart so the new interval applies from now rather than after the
    // remainder of the old one.
    if was_enabled {
        start(&app);
    }
    autosync_status(app)
}

/// Runs a scan immediately, without touching the schedule.
#[tauri::command]
pub async fn autosync_run_now(app: AppHandle) -> Result<AutoSyncStatus, String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || sync_once(&handle))
        .await
        .map_err(|e| e.to_string())??;
    autosync_status(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrecognised_interval_falls_back_to_the_default() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        put_setting(&conn, KEY_ENABLED, "1").unwrap();
        // A value written by an older build, or edited by hand, must not turn
        // into a one-minute loop.
        put_setting(&conn, KEY_INTERVAL, "1").unwrap();
        assert_eq!(read_config(&conn), (true, DEFAULT_INTERVAL));

        put_setting(&conn, KEY_INTERVAL, "360").unwrap();
        assert_eq!(read_config(&conn), (true, 360));
    }

    #[test]
    fn sync_is_off_until_the_user_turns_it_on() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        assert_eq!(read_config(&conn), (false, DEFAULT_INTERVAL));
    }

    #[test]
    fn granted_folders_are_reused_by_the_timer() {
        // A sandboxed build reaches ~/.claude only through a folder the user
        // picked by hand; a timed scan has no UI to ask again.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        put_setting(&conn, "agent_dir:claude-code", "/granted/claude").unwrap();
        put_setting(&conn, "agent_dir:codex", "").unwrap();
        put_setting(&conn, "unrelated", "x").unwrap();

        let overrides = dir_overrides(&conn);
        assert_eq!(overrides.get("claude-code").map(String::as_str), Some("/granted/claude"));
        assert!(!overrides.contains_key("codex"), "an empty override means 'use the default'");
        assert_eq!(overrides.len(), 1);
    }
}
