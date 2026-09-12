//! In-memory, one-shot approval relay for providers that expose a documented permission hook.
//!
//! Nothing is persisted: command arguments can contain secrets. A request expires after two
//! minutes, decisions are consumed once by the hook helper, and only `allow`/`deny`/`review` are
//! accepted. `review` means release the request to the provider's native permission dialog.

use serde::Serialize;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const REQUEST_TTL_MS: u64 = 120_000;
const DECISION_TTL_MS: u64 = 30_000;
const MAX_PENDING: usize = 32;

#[derive(Clone, Serialize, Debug)]
pub struct PendingApproval {
    pub id: String,
    pub provider: String,
    pub session_id: String,
    pub tool_name: String,
    pub summary: String,
    pub cwd: String,
    pub requested_at: u64,
    pub expires_at: u64,
}

struct Decision {
    value: String,
    at: u64,
}

#[derive(Default)]
pub struct ApprovalStore {
    pending: HashMap<String, PendingApproval>,
    decisions: HashMap<String, Decision>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn text(v: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .unwrap_or("")
        .to_string()
}

fn clipped(mut s: String, max: usize) -> String {
    s = s.replace('\0', "").replace('\r', " ").replace('\n', " ");
    if s.chars().count() > max {
        s = s.chars().take(max.saturating_sub(1)).collect::<String>() + "…";
    }
    s
}

fn preview(v: &serde_json::Value) -> String {
    for pointer in [
        "/tool_input/command",
        "/tool_input/file_path",
        "/tool_input/path",
        "/tool_input/url",
        "/input_preview",
        "/description",
    ] {
        if let Some(s) = v.pointer(pointer).and_then(|x| x.as_str()) {
            if !s.trim().is_empty() {
                return clipped(s.to_string(), 520);
            }
        }
    }
    v.get("tool_input")
        .and_then(|x| serde_json::to_string(x).ok())
        .map(|s| clipped(s, 520))
        .unwrap_or_else(|| "Review this action in the provider before approving.".into())
}

impl ApprovalStore {
    fn sweep(&mut self) {
        let now = now_ms();
        self.pending.retain(|_, p| p.expires_at > now);
        self.decisions
            .retain(|_, d| now.saturating_sub(d.at) <= DECISION_TTL_MS);
    }

    pub fn queue(&mut self, id: &str, provider: &str, body: &str) -> bool {
        self.sweep();
        if !matches!(provider, "claude")
            || id.len() < 8
            || id.len() > 96
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return false;
        }
        // A retry after a lost HTTP response must not replace the visible action or strand the
        // waiting hook. Treat the same opaque request id as an idempotent success.
        if self.pending.contains_key(id) {
            return true;
        }
        // Fail closed under an accidental hook storm instead of letting a local client grow this
        // memory-only queue without a bound. Existing requests can still be resolved normally.
        if self.pending.len() >= MAX_PENDING {
            return false;
        }
        let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
        let now = now_ms();
        let p = PendingApproval {
            id: id.to_string(),
            provider: provider.to_string(),
            session_id: clipped(text(&v, &["session_id", "conversation_id"]), 160),
            tool_name: clipped(text(&v, &["tool_name"]), 80),
            summary: preview(&v),
            cwd: clipped(text(&v, &["cwd"]), 260),
            requested_at: now,
            expires_at: now + REQUEST_TTL_MS,
        };
        self.pending.insert(id.to_string(), p);
        true
    }

    pub fn list(&mut self) -> Vec<PendingApproval> {
        self.sweep();
        let mut out: Vec<_> = self.pending.values().cloned().collect();
        out.sort_by(|a, b| b.requested_at.cmp(&a.requested_at));
        out
    }

    pub fn resolve(&mut self, id: &str, decision: &str) -> bool {
        self.sweep();
        if !matches!(decision, "allow" | "deny" | "review") || self.pending.remove(id).is_none() {
            return false;
        }
        self.decisions.insert(
            id.to_string(),
            Decision {
                value: decision.to_string(),
                at: now_ms(),
            },
        );
        true
    }

    pub fn take_result(&mut self, id: &str) -> String {
        self.sweep();
        if let Some(d) = self.decisions.remove(id) {
            d.value
        } else if self.pending.contains_key(id) {
            "pending".into()
        } else {
            "gone".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ApprovalStore;

    #[test]
    fn one_shot_decision_does_not_persist() {
        let mut s = ApprovalStore::default();
        assert!(s.queue(
            "claude-12345678",
            "claude",
            r#"{"session_id":"s","tool_name":"Bash","tool_input":{"command":"cargo test"}}"#,
        ));
        assert_eq!(s.list()[0].summary, "cargo test");
        assert!(s.resolve("claude-12345678", "allow"));
        assert_eq!(s.take_result("claude-12345678"), "allow");
        assert_eq!(s.take_result("claude-12345678"), "gone");
    }

    #[test]
    fn rejects_unknown_providers_and_bad_ids() {
        let mut s = ApprovalStore::default();
        assert!(!s.queue("short", "claude", "{}"));
        assert!(!s.queue("cursor-12345678", "cursor", "{}"));
    }

    #[test]
    fn duplicate_request_is_idempotent_and_review_is_one_shot() {
        let mut s = ApprovalStore::default();
        assert!(s.queue("claude-12345678", "claude", r#"{"tool_name":"Bash"}"#));
        assert!(s.queue("claude-12345678", "claude", r#"{"tool_name":"Write"}"#));
        assert_eq!(s.list()[0].tool_name, "Bash");
        assert!(s.resolve("claude-12345678", "review"));
        assert_eq!(s.take_result("claude-12345678"), "review");
        assert_eq!(s.take_result("claude-12345678"), "gone");
    }

    #[test]
    fn pending_queue_is_bounded() {
        let mut s = ApprovalStore::default();
        for i in 0..super::MAX_PENDING {
            assert!(s.queue(&format!("claude-{i:08}"), "claude", "{}"));
        }
        assert!(!s.queue("claude-overflow", "claude", "{}"));
    }
}
