//! `code-center.exe doctor deep`: deep diagnostics for finding "is it working?" signals.
//! Prints only structure, types and times. Scalar values and process command lines are deliberately
//! omitted so no token, account field, prompt, or conversation content can enter the report.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mtime_ms(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

/// Files modified within the last `within_s` seconds (depth-limited), sorted newest first
fn recent_files(root: &Path, depth: usize, within_s: u64, out: &mut Vec<(u64, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let now = now_ms();
    for e in rd.flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if p.is_dir() {
            if depth > 0
                && !name.starts_with("node_modules")
                && name != "Cache"
                && name != "Code Cache"
                && name != "GPUCache"
            {
                recent_files(&p, depth - 1, within_s, out);
            }
            continue;
        }
        if let Some(m) = mtime_ms(&p) {
            let age = now.saturating_sub(m) / 1000;
            if age <= within_s {
                out.push((age, p));
            }
        }
    }
}

fn shape(v: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value::*;
    match v {
        Null => "<null>".into(),
        Integer(_) => "<integer>".into(),
        Real(_) => "<real>".into(),
        Text(t) => format!("<text {} chars>", t.chars().count()),
        Blob(b) => format!("<blob {} bytes>", b.len()),
    }
}

/// Structure of one SQLite database plus, per table, the newest row's value shapes.
fn dump_sqlite(path: &Path) -> String {
    use rusqlite::OpenFlags;
    let mut o = format!(
        "--- {} ({}, modified {}s ago)\n",
        path.display(),
        if path.is_file() { "present" } else { "missing" },
        now_ms().saturating_sub(mtime_ms(path).unwrap_or(0)) / 1000
    );
    if !path.is_file() {
        return o;
    }
    let conn = match rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            o += &format!("  cannot open: {e}\n");
            return o;
        }
    };
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect()))
        .unwrap_or_default();
    for t in tables.iter().take(25) {
        let cols: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info(\"{t}\")"))
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))
            .unwrap_or(-1);
        o += &format!("  table {t} ({count} rows): {}\n", cols.join(", "));
        // Time-like columns: updated/created/_at/time/recency
        let timeish: Vec<&String> = cols
            .iter()
            .filter(|c| {
                let l = c.to_lowercase();
                l.contains("updated")
                    || l.contains("created")
                    || l.ends_with("_at")
                    || l.contains("time")
                    || l.contains("recency")
                    || l.contains("modified")
            })
            .collect();
        if let Some(tc) = timeish.first() {
            let sql = format!("SELECT * FROM \"{t}\" ORDER BY \"{tc}\" DESC LIMIT 1");
            if let Ok(mut s) = conn.prepare(&sql) {
                let n = s.column_count();
                if let Ok(mut rows) = s.query([]) {
                    if let Ok(Some(row)) = rows.next() {
                        let mut parts = Vec::new();
                        for i in 0..n {
                            let v: rusqlite::types::Value =
                                row.get(i).unwrap_or(rusqlite::types::Value::Null);
                            parts.push(format!(
                                "{}={}",
                                cols.get(i).cloned().unwrap_or_default(),
                                shape(&v)
                            ));
                        }
                        o += &format!("    newest row (by {tc}): {}\n", parts.join(" | "));
                    }
                }
            }
        }
    }
    o
}

/// JSON file: prints keys and value shapes only, never scalar values.
fn dump_json_scalars(path: &Path) -> String {
    let mut o = format!(
        "--- {} (modified {}s ago)\n",
        path.display(),
        now_ms().saturating_sub(mtime_ms(path).unwrap_or(0)) / 1000
    );
    let Ok(t) = std::fs::read_to_string(path) else {
        o += "  unreadable\n";
        return o;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else {
        o += "  not JSON\n";
        return o;
    };
    fn walk(v: &serde_json::Value, prefix: &str, depth: usize, o: &mut String) {
        if let Some(obj) = v.as_object() {
            for (k, x) in obj.iter().take(60) {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                match x {
                    serde_json::Value::Object(_) if depth < 2 => walk(x, &key, depth + 1, o),
                    serde_json::Value::Object(m) => {
                        o.push_str(&format!("  {key}: <object {} keys>\n", m.len()))
                    }
                    serde_json::Value::Array(a) => {
                        o.push_str(&format!("  {key}: <array {}>\n", a.len()))
                    }
                    serde_json::Value::String(s) => {
                        o.push_str(&format!("  {key}: <string {} chars>\n", s.chars().count()))
                    }
                    serde_json::Value::Number(_) => o.push_str(&format!("  {key}: <number>\n")),
                    serde_json::Value::Bool(_) => o.push_str(&format!("  {key}: <boolean>\n")),
                    serde_json::Value::Null => o.push_str(&format!("  {key}: <null>\n")),
                }
            }
        }
    }
    walk(&v, "", 0, &mut o);
    o
}

pub fn run() -> String {
    let mut o = String::from("== doctor deep: working-state signal survey ==\n(run it while both the Codex desktop app and the Claude desktop app are working)\n\n");
    let home = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();

    o += "## Files modified in the last 120 s\n";
    let mut recent = Vec::new();
    recent_files(&home.join(".codex"), 2, 120, &mut recent);
    if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.contains("claude") || n.contains("anthropic") {
                recent_files(
                    &e.path().join("LocalCache").join("Roaming").join("Claude"),
                    3,
                    120,
                    &mut recent,
                );
            }
        }
    }
    recent_files(
        &dirs::config_dir().unwrap_or_default().join("Claude"),
        2,
        120,
        &mut recent,
    );
    recent_files(
        &dirs::config_dir()
            .unwrap_or_default()
            .join("Cursor")
            .join("User")
            .join("globalStorage"),
        1,
        120,
        &mut recent,
    );
    recent.sort();
    for (age, p) in recent.iter().take(60) {
        o += &format!("  {age:>4}s ago  {}\n", p.display());
    }
    if recent.is_empty() {
        o += "  (none)\n";
    }

    o += "\n## Codex SQLite databases\n";
    for rel in [
        "state_5.sqlite",
        "thread_history_1.sqlite",
        "sqlite/codex-dev.db",
        "goals_1.sqlite",
        "queue_1.sqlite",
    ] {
        o += &dump_sqlite(&home.join(".codex").join(rel));
    }

    o += "\n## Codex global state JSON (scalar keys only)\n";
    o += &dump_json_scalars(&home.join(".codex").join(".codex-global-state.json"));

    o += "\n## Last line of Codex session_index.jsonl (key names)\n";
    if let Ok(t) = std::fs::read_to_string(home.join(".codex").join("session_index.jsonl")) {
        if let Some(last) = t.lines().rev().find(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<serde_json::Value>(last) {
                Ok(v) => {
                    let keys: Vec<String> = v
                        .as_object()
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                    o += &format!("  keys: {}\n", keys.join(", "));
                }
                Err(_) => o += "  not JSON\n",
            }
        }
    }

    o += "\n## Codex processes (PID and executable name only)\n";
    #[cfg(windows)]
    {
        let maps = crate::focus::proc_maps();
        let mut rows: Vec<_> = maps
            .name
            .iter()
            .filter(|(_, name)| name.starts_with("codex") || name.as_str() == "chatgpt.exe")
            .map(|(pid, name)| (*pid, name.clone()))
            .collect();
        rows.sort_by_key(|(pid, _)| *pid);
        for (pid, name) in rows {
            o += &format!("  {pid}\t{name}\n");
        }
    }
    o
}
