//! GitHub Copilot presence and account-quota adapter.
//!
//! Quota comes from the official Copilot SDK's experimental `account.getQuota` RPC. Codenotch
//! starts the user's existing Copilot CLI briefly, reads only entitlement counters, then shuts it
//! down. The CLI is not bundled, credentials are never read by Codenotch, and no prompt/session is
//! created. If GitHub changes this experimental shape, the adapter degrades to an unavailable note.

use crate::usage::{LimitWindow, UsageSnapshot};
use crate::AppState;
use github_copilot_sdk::{CliProgram, Client, ClientInfo, ClientOptions, LogLevel};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const POLL_SECS: u64 = 300;
static REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn request_refresh() {
    REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn store_path() -> PathBuf {
    crate::config::config_path().with_file_name("copilot.json")
}

pub fn load_persisted() -> UsageSnapshot {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str::<UsageSnapshot>(&t).ok())
        .map(|mut s| {
            if !s.windows.is_empty() {
                s.status = "stale".into();
            }
            s
        })
        .unwrap_or_default()
}

fn persist(s: &UsageSnapshot) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(store_path(), t);
    }
}

fn vscode_extension_present() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    for root in [
        home.join(".vscode").join("extensions"),
        home.join(".vscode-insiders").join("extensions"),
    ] {
        if let Ok(rd) = std::fs::read_dir(root) {
            if rd.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                n.starts_with("github.copilot-") || n.starts_with("github.copilot-chat-")
            }) {
                return true;
            }
        }
    }
    false
}

pub fn find_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".copilot").join("bin").join("copilot.exe"));
    }
    if let Some(local) = dirs::data_local_dir() {
        candidates.push(
            local
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("copilot.exe"),
        );
        candidates.push(
            local
                .join("Programs")
                .join("GitHub Copilot")
                .join("copilot.exe"),
        );
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join("copilot.exe"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn present() -> bool {
    find_executable().is_some() || vscode_extension_present() || copilot_app_present()
}

#[cfg(windows)]
fn copilot_app_present() -> bool {
    crate::focus::proc_maps()
        .name
        .values()
        .any(|n| matches!(n.as_str(), "githubcopilot.exe" | "github copilot.exe"))
}

#[cfg(not(windows))]
fn copilot_app_present() -> bool {
    false
}

fn parse_iso(v: Option<&serde_json::Value>) -> Option<u64> {
    v.and_then(|x| x.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis().max(0) as u64)
}

fn quota_window(id: &str, label: &str, v: &serde_json::Value) -> Option<LimitWindow> {
    let entitlement = v
        .get("entitlementRequests")
        .and_then(|x| x.as_f64())
        .unwrap_or(-1.0);
    let used_requests = v
        .get("usedRequests")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let used = if entitlement > 0.0 {
        used_requests / entitlement
    } else {
        let left = v.get("remainingPercentage").and_then(|x| x.as_f64())?;
        (100.0 - left) / 100.0
    };
    Some(LimitWindow {
        id: id.into(),
        label: label.into(),
        used: used.clamp(0.0, 1.0),
        resets_at: parse_iso(v.get("resetDate")),
        ..Default::default()
    })
}

fn parse_quota(v: &serde_json::Value) -> (Vec<LimitWindow>, String) {
    let q = v.get("quotaSnapshots").unwrap_or(v);
    let mut windows = Vec::new();
    for (key, label) in [
        ("premium_interactions", "Premium requests"),
        ("ai_credits", "AI credits"),
        ("chat", "Chat"),
        ("completions", "Completions"),
    ] {
        if let Some(raw) = q.get(key) {
            if let Some(w) = quota_window(key, label, raw) {
                windows.push(w);
            }
        }
    }
    let note = if windows.is_empty() {
        "Copilot is signed in, but this plan reports no finite account quota.".into()
    } else {
        "GitHub Copilot SDK · experimental quota API".into()
    };
    (windows, note)
}

fn fetch() -> Result<UsageSnapshot, String> {
    let cli = find_executable()
        .ok_or("Copilot found, but Copilot CLI is not installed; quota is unavailable")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let info = ClientInfo::new()
            .with_application_name("Codenotch")
            .with_application_version(env!("CARGO_PKG_VERSION"))
            .with_integration_name("usage-meter");
        let options = ClientOptions::new()
            .with_program(CliProgram::Path(cli))
            .with_log_level(LogLevel::Error)
            .with_client_info(info);
        let client = tokio::time::timeout(Duration::from_secs(15), Client::start(options))
            .await
            .map_err(|_| "Copilot CLI startup timed out".to_string())?
            .map_err(|e| e.to_string())?;
        let reply = match tokio::time::timeout(
            Duration::from_secs(12),
            client.call("account.getQuota", Some(serde_json::json!({}))),
        )
        .await
        {
            Ok(reply) => reply,
            Err(_) => {
                client.force_stop();
                return Err("Copilot quota request timed out".into());
            }
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(3), client.stop()).await,
            Ok(Ok(()))
        ) {
            client.force_stop();
        }
        let value = reply.map_err(|e| e.to_string())?;
        let (windows, note) = parse_quota(&value);
        Ok(UsageSnapshot {
            status: if windows.is_empty() {
                "none".into()
            } else {
                "ok".into()
            },
            windows,
            fetched_at: now_ms(),
            note,
            ..Default::default()
        })
    })
}

fn read_once(prev: &UsageSnapshot) -> UsageSnapshot {
    if !present() {
        return UsageSnapshot {
            status: "absent".into(),
            ..Default::default()
        };
    }
    match fetch() {
        Ok(s) => s,
        Err(e) => {
            let mut s = prev.clone();
            let lower = e.to_ascii_lowercase();
            s.status = if lower.contains("auth") || lower.contains("sign in") {
                "needsAuth"
            } else if s.windows.is_empty() {
                "none"
            } else {
                "stale"
            }
            .into();
            s.note = e;
            s
        }
    }
}

fn broadcast(app: &AppHandle, snap: UsageSnapshot) {
    let st = app.state::<AppState>();
    *st.copilot.lock().unwrap() = snap.clone();
    persist(&snap);
    let _ = app.emit("copilot", &snap);
}

fn sleep_interruptible(secs: u64) {
    for _ in 0..secs {
        if REFRESH.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        let prev = {
            let st = app.state::<AppState>();
            let snapshot = st.copilot.lock().unwrap().clone();
            snapshot
        };
        let snap = read_once(&prev);
        if matches!(snap.status.as_str(), "stale" | "error") {
            crate::applog(&format!("copilot: {}", snap.note));
        }
        broadcast(&app, snap);
        sleep_interruptible(if present() { POLL_SECS } else { 600 });
    });
}

#[cfg(test)]
mod tests {
    use super::parse_quota;

    #[test]
    fn parses_official_account_quota_shape() {
        let v = serde_json::json!({"quotaSnapshots":{"premium_interactions":{
            "entitlementRequests":300,"usedRequests":75,"remainingPercentage":75.0,
            "resetDate":"2026-10-01T00:00:00Z"
        }}});
        let (w, _) = parse_quota(&v);
        assert_eq!(w.len(), 1);
        assert!((w[0].used - 0.25).abs() < 0.001);
        assert!(w[0].resets_at.is_some());
    }
}
