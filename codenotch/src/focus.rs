//! Jump back to the right terminal: from the session's Claude CLI process PID, walk the parent chain
//! to the hosting terminal window, then SetForegroundWindow + FlashWindowEx. Returns false on failure (the page reports it).

#[cfg(windows)]
pub fn focus_terminal(claude_pid: u32) -> bool {
    use std::collections::HashMap;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FlashWindowEx, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, ShowWindow, FLASHWINFO, FLASHW_ALL, SW_RESTORE,
    };

    if claude_pid == 0 {
        return false;
    }

    // 1) Full pid -> ppid snapshot
    let mut ppid_map: HashMap<u32, u32> = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                ppid_map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }

    // 2) claude's ancestor chain (itself included), at most 8 levels: node → shell → WindowsTerminal/conhost host…
    let mut chain: Vec<u32> = vec![claude_pid];
    let mut cur = claude_pid;
    for _ in 0..8 {
        match ppid_map.get(&cur) {
            Some(&p) if p != 0 && !chain.contains(&p) => {
                chain.push(p);
                cur = p;
            }
            _ => break,
        }
    }

    // 3) Enumerate visible top-level windows
    struct Cand {
        hwnd: isize,
        pid: u32,
    }
    let mut wins: Vec<Cand> = Vec::new();
    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l.0 as *mut Vec<(isize, u32)>);
        if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            v.push((hwnd.0 as isize, pid));
        }
        BOOL(1)
    }
    let mut raw: Vec<(isize, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut raw as *mut _ as isize));
    }
    for (h, p) in raw {
        wins.push(Cand { hwnd: h, pid: p });
    }

    // 4) Score: the window's PID is on the ancestor chain (higher up = the real terminal host = higher
    //    score), or the window PID's parent is on the chain (the classic conhost case).
    let score_of = |pid: u32| -> Option<usize> {
        if let Some(i) = chain.iter().position(|&c| c == pid) {
            return Some(i);
        }
        if let Some(&pp) = ppid_map.get(&pid) {
            if let Some(i) = chain.iter().position(|&c| c == pp) {
                return Some(i);
            }
        }
        None
    };
    let best = wins
        .iter()
        .filter_map(|w| score_of(w.pid).map(|s| (s, w.hwnd)))
        .max_by_key(|(s, _)| *s);

    let Some((_, hwnd_raw)) = best else {
        return false;
    };
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        let fi = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_ALL,
            uCount: 2,
            dwTimeout: 0,
        };
        let _ = FlashWindowEx(&fi);
    }
    true
}

#[cfg(not(windows))]
pub fn focus_terminal(_claude_pid: u32) -> bool {
    false
}

// ---------------- Process and foreground helpers shared by seen-clears-it and the desktop jump-back ----------------

#[cfg(windows)]
pub struct ProcMaps {
    pub ppid: std::collections::HashMap<u32, u32>,
    pub name: std::collections::HashMap<u32, String>, // lower-case exe name
}

#[cfg(windows)]
pub fn proc_maps() -> ProcMaps {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut m = ProcMaps {
        ppid: Default::default(),
        name: Default::default(),
    };
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return m;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                m.ppid.insert(e.th32ProcessID, e.th32ParentProcessID);
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                m.name.insert(
                    e.th32ProcessID,
                    String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase(),
                );
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
    }
    m
}

/// Process ids that own a real user-facing application window. Electron apps commonly leave
/// helper processes alive after their main window closes; process presence alone therefore cannot
/// mean "open" in Smart notch mode. Minimized windows still count, while tiny helper/overlay
/// windows do not.
#[cfg(windows)]
pub fn app_window_pids() -> std::collections::HashSet<u32> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible,
    };

    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> BOOL {
        let pids = &mut *(l.0 as *mut std::collections::HashSet<u32>);
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) <= 0 {
            return BOOL(1);
        }
        let mut rect = RECT::default();
        let substantial = GetWindowRect(hwnd, &mut rect).is_ok()
            && (rect.right - rect.left).max(0) >= 240
            && (rect.bottom - rect.top).max(0) >= 160;
        if substantial || IsIconic(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                pids.insert(pid);
            }
        }
        BOOL(1)
    }

    let mut pids = std::collections::HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut pids as *mut _ as isize));
    }
    pids
}

#[cfg(windows)]
pub fn fg_pid() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return 0;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    }
}

#[cfg(windows)]
pub fn chain_of(pid: u32, ppid: &std::collections::HashMap<u32, u32>) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut cur = pid;
    for _ in 0..8 {
        match ppid.get(&cur) {
            Some(&p) if p != 0 && !chain.contains(&p) => {
                chain.push(p);
                cur = p;
            }
            _ => break,
        }
    }
    chain
}

/// Whether the foreground process belongs to a session's terminal window (itself on the chain, or its parent — the conhost case)
#[cfg(windows)]
pub fn pid_hits_chain(pid: u32, chain: &[u32], maps: &ProcMaps) -> bool {
    chain.contains(&pid)
        || maps
            .ppid
            .get(&pid)
            .map(|p| chain.contains(p))
            .unwrap_or(false)
}

/// Focus the Claude desktop app's main window (the jump-back target for desktop sessions: the largest visible window whose process name contains claude)
#[cfg(windows)]
pub fn focus_claude_desktop() -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FlashWindowEx, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, FLASHWINFO, FLASHW_ALL,
        SW_RESTORE,
    };
    let maps = proc_maps();
    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l.0 as *mut Vec<(isize, u32)>);
        if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            v.push((hwnd.0 as isize, pid));
        }
        BOOL(1)
    }
    let mut wins: Vec<(isize, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut wins as *mut _ as isize));
    }
    let mut best: Option<(isize, i64)> = None;
    for (h, pid) in wins {
        let Some(name) = maps.name.get(&pid) else {
            continue;
        };
        if !name.contains("claude") || name.contains("codenotch") {
            continue;
        }
        let mut r = RECT::default();
        let area = unsafe {
            if GetWindowRect(HWND(h as *mut core::ffi::c_void), &mut r).is_ok() {
                ((r.right - r.left) as i64) * ((r.bottom - r.top) as i64)
            } else {
                0
            }
        };
        if best.map(|(_, a)| area > a).unwrap_or(true) {
            best = Some((h, area));
        }
    }
    let Some((h, _)) = best else {
        return false;
    };
    unsafe {
        let hwnd = HWND(h as *mut core::ffi::c_void);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        let fi = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_ALL,
            uCount: 2,
            dwTimeout: 0,
        };
        let _ = FlashWindowEx(&fi);
    }
    true
}

#[cfg(not(windows))]
pub fn focus_claude_desktop() -> bool {
    false
}

/// Focus the largest visible window belonging to a provider. Codex Desktop's packaged process is
/// named ChatGPT.exe and its local agent children are codex.exe; the other providers keep their
/// product name in the executable. Window titles are used as a secondary signal because packaged
/// Windows apps sometimes expose their top-level window through a host process.
#[cfg(windows)]
pub fn focus_provider(provider: &str) -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FlashWindowEx, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
        FLASHWINFO, FLASHW_ALL, SW_RESTORE,
    };

    if provider == "claude" && focus_claude_desktop() {
        return true;
    }
    let maps = proc_maps();
    let provider = provider.to_lowercase();
    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l.0 as *mut Vec<(isize, u32, String)>);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; (len.max(0) + 1) as usize];
        let got = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..got.max(0) as usize]).to_lowercase();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        v.push((hwnd.0 as isize, pid, title));
        BOOL(1)
    }
    let mut wins: Vec<(isize, u32, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut wins as *mut _ as isize));
    }

    let matches = |name: &str, title: &str| match provider.as_str() {
        "codex" => name == "chatgpt.exe" || name == "codex.exe" || title.contains("codex"),
        "cursor" => name.contains("cursor") || title.contains("cursor"),
        "copilot" => {
            name == "githubcopilot.exe"
                || name == "github copilot.exe"
                || name == "code.exe"
                || title.contains("copilot")
        }
        "gemini" => name.contains("antigravity") || title.contains("antigravity"),
        "claude" => name.contains("claude") || title.contains("claude"),
        _ => false,
    };
    let mut best: Option<(isize, i64)> = None;
    for (h, pid, title) in wins {
        let name = maps.name.get(&pid).map(String::as_str).unwrap_or("");
        if name.contains("codenotch") || !matches(name, &title) {
            continue;
        }
        let mut r = RECT::default();
        let area = unsafe {
            if GetWindowRect(HWND(h as *mut core::ffi::c_void), &mut r).is_ok() {
                ((r.right - r.left).max(0) as i64) * ((r.bottom - r.top).max(0) as i64)
            } else {
                0
            }
        };
        if best.map(|(_, a)| area > a).unwrap_or(true) {
            best = Some((h, area));
        }
    }
    let Some((h, _)) = best else { return false };
    unsafe {
        let hwnd = HWND(h as *mut core::ffi::c_void);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let focused = SetForegroundWindow(hwnd).as_bool();
        let fi = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_ALL,
            uCount: 2,
            dwTimeout: 0,
        };
        let _ = FlashWindowEx(&fi);
        focused
    }
}

#[cfg(not(windows))]
pub fn focus_provider(_provider: &str) -> bool {
    false
}
