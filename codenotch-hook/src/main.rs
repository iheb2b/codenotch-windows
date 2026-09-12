//! codenotch-hook: the minimal client Claude Code's hooks call.
//! Duties: 1) report activity plus stdin JSON to the main app; 2) for Claude Code's documented
//! PermissionRequest hook, wait for one explicit one-shot decision; 3) launch the app if needed.
//! Failures never approve anything: a failed/timed-out relay prints no decision, leaving Claude's
//! own permission UI in charge.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const DEFAULT_PORT: u16 = 48666;
const MAX_STDIN: u64 = 256 * 1024;
const ALLOW_OUTPUT: &str = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#;
const DENY_OUTPUT: &str = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny"}}}"#;

fn main() {
    let event = std::env::args().nth(1).unwrap_or_else(|| "ping".into());

    // The hook's stdin is the JSON Claude Code provides (session_id / cwd / prompt / message…)
    let mut body = String::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_string(&mut body);

    let (mut port, mut token) = read_config();
    let ppid = parent_pid();

    if event == "approval-claude" {
        approval(port, token, ppid, &body);
        return;
    }

    if send(port, &token, &event, ppid, &body).is_ok() {
        return;
    }
    // Main app not running: launch it detached, then retry briefly
    spawn_main();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        (port, token) = read_config();
        if send(port, &token, &event, ppid, &body).is_ok() {
            return;
        }
    }
    // Give up quietly — never affect Claude Code
}

/// Pulls two primitive fields out of config.json without adding a JSON dependency to this tiny
/// sidecar. The token contains lowercase hex only, by construction.
fn read_config() -> (u16, String) {
    let path = match std::env::var("APPDATA") {
        Ok(a) => format!("{a}\\codenotch\\config.json"),
        Err(_) => return (DEFAULT_PORT, String::new()),
    };
    let Ok(txt) = std::fs::read_to_string(path) else {
        return (DEFAULT_PORT, String::new());
    };
    let mut port = DEFAULT_PORT;
    if let Some(i) = txt.find("\"port\"") {
        let digits: String = txt[i + 6..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        port = digits.parse().unwrap_or(DEFAULT_PORT);
    }
    let token = string_field(&txt, "bridge_token");
    let token = if token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        token
    } else {
        String::new()
    };
    (port, token)
}

fn string_field(txt: &str, key: &str) -> String {
    let needle = format!("\"{key}\"");
    let Some(i) = txt.find(&needle) else {
        return String::new();
    };
    let tail = &txt[i + needle.len()..];
    let Some(colon) = tail.find(':') else {
        return String::new();
    };
    let tail = tail[colon + 1..].trim_start();
    let Some(tail) = tail.strip_prefix('"') else {
        return String::new();
    };
    tail.chars().take_while(|c| *c != '"').collect()
}

fn send(port: u16, token: &str, event: &str, ppid: u32, body: &str) -> std::io::Result<()> {
    let path = format!("/event?e={event}&ppid={ppid}");
    request(port, token, "POST", &path, body).map(|_| ())
}

fn request(
    port: u16,
    token: &str,
    method: &str,
    path: &str,
    body: &str,
) -> std::io::Result<String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    s.set_write_timeout(Some(Duration::from_millis(700)))?;
    s.set_read_timeout(Some(Duration::from_millis(700)))?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Codenotch-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len(),
    );
    s.write_all(req.as_bytes())?;
    let mut reply = String::new();
    s.take(16 * 1024).read_to_string(&mut reply)?;
    let status = reply
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if !(200..300).contains(&status) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("local bridge returned HTTP {status}"),
        ));
    }
    Ok(reply
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string())
}

fn approval(mut port: u16, mut token: String, ppid: u32, body: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let id = format!("claude-{}-{now}", std::process::id());
    let path = format!("/approval?provider=claude&id={id}&ppid={ppid}");
    let mut queued = token.len() >= 32
        && request(port, &token, "POST", &path, body).ok().as_deref() == Some("queued");
    if !queued {
        spawn_main();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            (port, token) = read_config();
            if token.len() >= 32
                && request(port, &token, "POST", &path, body).ok().as_deref() == Some("queued")
            {
                queued = true;
                break;
            }
        }
    }
    if !queued {
        return;
    }
    let result_path = format!("/approval-result?id={id}");
    for _ in 0..460 {
        std::thread::sleep(Duration::from_millis(250));
        match request(port, &token, "GET", &result_path, "")
            .ok()
            .as_deref()
        {
            Some("allow") => {
                println!("{ALLOW_OUTPUT}");
                return;
            }
            Some("deny") => {
                println!("{DENY_OUTPUT}");
                return;
            }
            // Print no decision: Claude Code proceeds to its own full permission dialog.
            Some("review") => return,
            Some("gone") => return,
            _ => {}
        }
    }
}

/// Launches the main app detached: no inherited handles, no window, never waits
fn spawn_main() {
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = me.parent() else { return };
    let exe = dir.join("codenotch.exe");
    if !exe.exists() {
        return;
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    let _ = cmd.spawn();
}

/// Parent process PID (≈ the Claude Code CLI process) via NtQueryInformationProcess, no dependency
#[cfg(windows)]
fn parent_pid() -> u32 {
    #[repr(C)]
    struct Pbi {
        exit_status: isize,
        peb: usize,
        affinity_mask: usize,
        base_priority: isize,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }
    extern "system" {
        fn NtQueryInformationProcess(
            handle: isize,
            class: u32,
            info: *mut Pbi,
            len: u32,
            ret_len: *mut u32,
        ) -> i32;
    }
    unsafe {
        let mut pbi = std::mem::zeroed::<Pbi>();
        let mut ret = 0u32;
        // -1 = GetCurrentProcess()
        if NtQueryInformationProcess(-1, 0, &mut pbi, std::mem::size_of::<Pbi>() as u32, &mut ret)
            == 0
        {
            return pbi.inherited_from_unique_process_id as u32;
        }
    }
    0
}

#[cfg(not(windows))]
fn parent_pid() -> u32 {
    std::os::unix::process::parent_id()
}
