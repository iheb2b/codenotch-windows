//! Start at sign-in: an HKCU\...\Run registry value (per user, no administrator needed).
//! The command carries --silent: wait in the background, show no bar without sessions, appear when one starts.
//! Implemented with reg.exe, so no new dependency.

use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "Code Center";
const LEGACY_NAME: &str = "Codenotch";

fn reg(args: &[&str]) -> Option<(bool, String)> {
    let mut c = Command::new("reg");
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.output().ok().map(|o| {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        (o.status.success(), text)
    })
}

pub fn is_enabled() -> bool {
    [NAME, LEGACY_NAME].iter().any(|name| {
        reg(&["query", RUN_KEY, "/v", name])
            .map(|(ok, out)| ok && out.contains(name))
            .unwrap_or(false)
    })
}

pub fn enable() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let val = format!("\"{}\" --silent", exe.display());
    match reg(&["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &val, "/f"]) {
        Some((true, _)) => {
            // Remove the former value only after the replacement succeeds.
            let _ = reg(&["delete", RUN_KEY, "/v", LEGACY_NAME, "/f"]);
            Ok("start at sign-in enabled (silent until a session appears)".into())
        }
        Some((false, out)) => Err(out),
        None => Err("reg.exe failed to run".into()),
    }
}

pub fn disable() -> Result<String, String> {
    let mut removed = false;
    let mut failure = None;
    for name in [NAME, LEGACY_NAME] {
        match reg(&["delete", RUN_KEY, "/v", name, "/f"]) {
            Some((true, _)) => removed = true,
            Some((false, out)) => {
                // reg.exe answers in the OS language; "找不到" is "unable to find".
                if !out.to_lowercase().contains("unable to find") && !out.contains("找不到") {
                    failure = Some(out);
                }
            }
            None => return Err("reg.exe failed to run".into()),
        }
    }
    match failure {
        Some(out) => Err(out),
        None if removed => Ok("start at sign-in disabled".into()),
        None => Ok("start at sign-in was not enabled".into()),
    }
}
