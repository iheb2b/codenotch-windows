//! Merges codenotch-hook.exe into ~/.claude/settings.json without overwriting the user's own hooks.
//! Identification: the command contains "codenotch-hook". A backup is written first.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// (Claude Code event name, whether it needs a matcher, internal event, timeout seconds).
/// PermissionRequest is the only synchronous entry: it waits for a one-shot decision from the
/// notch, then returns Claude Code's documented allow/deny JSON. Every other event stays tiny.
const WIRING: &[(&str, bool, &str, u64)] = &[
    ("SessionStart", false, "session_start", 5),
    ("UserPromptSubmit", false, "running", 5),
    ("PreToolUse", true, "running", 5),
    ("PostToolUse", true, "running", 5),
    ("PermissionRequest", true, "approval-claude", 125),
    ("Notification", false, "attention", 5),
    ("Stop", false, "done", 5),
    ("SessionEnd", false, "session_end", 5),
];

fn settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn is_ours(entry: &Value) -> bool {
    entry["hooks"]
        .as_array()
        .map(|hs| {
            hs.iter().any(|h| {
                h["command"]
                    .as_str()
                    .map(|command| {
                        command
                            .split(|c: char| c == '"' || c.is_whitespace())
                            .filter(|part| !part.is_empty())
                            .filter_map(|part| Path::new(part).file_name())
                            .map(|name| name.to_string_lossy().to_ascii_lowercase())
                            .any(|name| {
                                matches!(
                                    name.as_str(),
                                    "codenotch-hook.exe"
                                        | "codenotch-hook"
                                        | "eatbean-hook.exe"
                                        | "eatbean-hook"
                                        | "pacman-hook.exe"
                                        | "pacman-hook"
                                )
                            })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn load(path: &PathBuf) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}; nothing was changed", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("cannot parse {}: {e}; nothing was changed", path.display()))
}

fn backup_and_write(path: &PathBuf, root: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = std::fs::copy(
            path,
            path.with_extension(format!("json.codenotch-bak-{ts}")),
        );
    }
    let txt = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    std::fs::write(path, txt).map_err(|e| e.to_string())
}

pub fn is_installed() -> bool {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        // Older releases installed activity-only hooks. Keep the switch off until the user opts
        // into the new approval bridge explicitly; upgrading must not broaden permissions silently.
        .map(|t| {
            let t = t.to_ascii_lowercase();
            t.contains("codenotch-hook") && t.contains("approval-claude")
        })
        .unwrap_or(false)
}

pub fn install() -> Result<String, String> {
    let path = settings_path().ok_or("cannot find the user directory")?;
    let hook_exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("cannot locate the program directory")?
        .join("codenotch-hook.exe");
    if !hook_exe.exists() {
        return Err(format!("missing {}", hook_exe.display()));
    }

    let mut root = load(&path)?;
    if !root.is_object() {
        return Err(format!(
            "{} must contain a JSON object; nothing was changed",
            path.display()
        ));
    }
    if let Some(hooks) = root.get("hooks") {
        if !hooks.is_null() && !hooks.is_object() {
            return Err(format!(
                "the hooks entry in {} must be a JSON object; nothing was changed",
                path.display()
            ));
        }
    }
    if root.get("hooks").map(Value::is_null).unwrap_or(true) {
        root["hooks"] = json!({});
    }

    for (event, need_matcher, internal, timeout) in WIRING {
        let arr = match root["hooks"].get(*event) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items.clone(),
            Some(_) => {
                return Err(format!(
                    "the {event} hook entry in {} must be an array; nothing was changed",
                    path.display()
                ));
            }
        };
        // Remove our own older entries first
        let mut arr: Vec<Value> = arr.into_iter().filter(|e| !is_ours(e)).collect();
        let cmd = format!("\"{}\" {}", hook_exe.display(), internal);
        let mut entry = json!({
            "hooks": [{ "type": "command", "command": cmd, "timeout": timeout }]
        });
        if *need_matcher {
            entry["matcher"] = json!("*");
        }
        arr.push(entry);
        root["hooks"][*event] = json!(arr);
    }

    backup_and_write(&path, &root)?;
    Ok(format!(
        "wrote {} ({} events, including one-shot approvals)",
        path.display(),
        WIRING.len()
    ))
}

pub fn uninstall() -> Result<String, String> {
    let path = settings_path().ok_or("cannot find the user directory")?;
    if !path.exists() {
        return Ok("settings.json does not exist, nothing to uninstall".into());
    }
    let mut root = load(&path)?;
    let Some(hooks) = root["hooks"].as_object_mut() else {
        return Ok("no hooks configuration found".into());
    };
    let mut removed = 0;
    for (_, v) in hooks.iter_mut() {
        if let Some(arr) = v.as_array() {
            let filtered: Vec<Value> = arr.iter().filter(|e| !is_ours(e)).cloned().collect();
            removed += arr.len() - filtered.len();
            *v = json!(filtered);
        }
    }
    backup_and_write(&path, &root)?;
    Ok(format!("removed {removed} Codenotch hook(s)"))
}

#[cfg(test)]
mod tests {
    use super::{is_ours, load, WIRING};
    use serde_json::json;

    #[test]
    fn identifies_our_current_and_legacy_hooks_without_claiming_others() {
        assert!(is_ours(
            &json!({"hooks":[{"command":"C:\\Apps\\codenotch-hook.exe running"}]})
        ));
        assert!(is_ours(
            &json!({"hooks":[{"command":"C:\\Apps\\Codenotch-Hook.exe running"}]})
        ));
        assert!(is_ours(
            &json!({"hooks":[{"command":"pacman-hook.exe done"}]})
        ));
        assert!(!is_ours(
            &json!({"hooks":[{"command":"my-company-hook.exe"}]})
        ));
        assert!(!is_ours(
            &json!({"hooks":[{"command":"my-codenotch-hook.exe running"}]})
        ));
    }

    #[test]
    fn permission_hook_is_unique_and_allows_the_relay_timeout() {
        let permission: Vec<_> = WIRING
            .iter()
            .filter(|(event, _, _, _)| *event == "PermissionRequest")
            .collect();
        assert_eq!(permission.len(), 1);
        assert_eq!(permission[0].2, "approval-claude");
        assert!(permission[0].3 >= 120);
    }

    #[test]
    fn malformed_settings_are_rejected_without_replacement() {
        let path = std::env::temp_dir().join(format!(
            "codenotch-hooks-invalid-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let original = b"{ definitely not valid json";
        std::fs::write(&path, original).unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_file(path);
    }
}
