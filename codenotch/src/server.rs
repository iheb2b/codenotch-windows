//! Local hook server. Activity events stay fire-and-forget; documented provider permission hooks
//! use a short-polling, one-shot approval relay. Every request needs a custom token header so a web
//! page cannot manufacture a prompt through a no-CORS localhost POST.

use crate::state::HookEvent;
use crate::AppState;
use std::io::Read;
use tauri::{AppHandle, Emitter, Manager};

pub fn start(app: AppHandle, port: u16) {
    std::thread::spawn(move || {
        let server = match tiny_http::Server::http(("127.0.0.1", port)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[codenotch] failed to bind port {port}: {e} (is another instance running?)"
                );
                return;
            }
        };
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            let route = url.split('?').next().unwrap_or("");
            let method_ok = matches!(
                (route, req.method()),
                ("/event", &tiny_http::Method::Post)
                    | ("/approval", &tiny_http::Method::Post)
                    | ("/approval-result", &tiny_http::Method::Get)
            );
            if !matches!(route, "/event" | "/approval" | "/approval-result") {
                let _ = req
                    .respond(tiny_http::Response::from_string("not found").with_status_code(404));
                continue;
            }
            if !method_ok {
                let _ = req.respond(
                    tiny_http::Response::from_string("method not allowed").with_status_code(405),
                );
                continue;
            }
            let supplied = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("X-Codenotch-Token"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let expected = {
                let state = app.state::<AppState>();
                state.cfg.lock().unwrap().bridge_token.clone()
            };
            if expected.is_empty() || supplied != expected {
                let _ = req
                    .respond(tiny_http::Response::from_string("forbidden").with_status_code(403));
                continue;
            }
            let mut body = String::new();
            let _ = req.as_reader().take(256 * 1024).read_to_string(&mut body);
            let mut reply = "ok".to_string();
            if route == "/event" {
                let ev = parse(&url, &body);
                let state = app.state::<AppState>();
                let changed = {
                    let mut store = state.store.lock().unwrap();
                    store.apply(ev)
                };
                if changed {
                    crate::broadcast(&app);
                }
            } else if route == "/approval-result" {
                let id = query_param(&url, "id");
                let state = app.state::<AppState>();
                reply = state.approvals.lock().unwrap().take_result(&id);
            } else if route == "/approval" {
                let id = query_param(&url, "id");
                let provider = query_param(&url, "provider");
                let accepted = {
                    let state = app.state::<AppState>();
                    state.approvals.lock().unwrap().queue(&id, &provider, &body)
                };
                if accepted {
                    let pending = {
                        let state = app.state::<AppState>();
                        state.approvals.lock().unwrap().list()
                    };
                    let _ = app.emit("approvals", pending);
                }
                reply = if accepted { "queued" } else { "rejected" }.into();
            }
            let _ = req.respond(tiny_http::Response::from_string(reply));
        }
    });
}

fn query_param(url: &str, key: &str) -> String {
    let q = url.splitn(2, '?').nth(1).unwrap_or("");
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return it.next().unwrap_or("").to_string();
        }
    }
    String::new()
}

fn parse(url: &str, body: &str) -> HookEvent {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    // tool_input.command (Bash etc.) feeds the "last action" summary
    let tool_cmd = v
        .get("tool_input")
        .and_then(|t| t.get("command"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    HookEvent {
        e: query_param(url, "e"),
        session_id: {
            let id = s("session_id");
            if id.is_empty() {
                "unknown".into()
            } else {
                id
            }
        },
        ppid: query_param(url, "ppid").parse().unwrap_or(0),
        cwd: s("cwd"),
        prompt: s("prompt"),
        message: s("message"),
        tool_name: s("tool_name"),
        tool_cmd,
        model: s("model"),
        src: "hook",
    }
}
