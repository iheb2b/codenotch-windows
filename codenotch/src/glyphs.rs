//! Provider glyphs.
//!
//! Rule: **no vendor logo is drawn by hand here**; only existing artwork is used, in this order:
//!   1. User override: `%APPDATA%\codenotch\glyphs\<id>.svg|.png`, or `glyphs\` next to the exe;
//!   2. Built in: original provider artwork compiled into the exe from each provider's official
//!      press/brand kit (and the installed Codex Windows package); provenance in glyphs/NOTICE.md;
//!   3. The installed/running application's own icon (PrivateExtractIconsW on the exe resources,
//!      64 px → PNG) only when built-in artwork is unavailable;
//!   none of those → the page falls back to a letter.
//! Trusted built-in SVGs are inlined into the DOM; user SVGs, PNGs and app icons go through <img>
//! so user-provided markup never enters the document. Official brand icons use `brandicon` so the
//! UI never recolours or redraws them. Ids match the page and upstream:
//! claude / codex / cursor / copilot / gemini.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Debug, Default)]
pub struct Glyph {
    /// svg = trusted inline SVG; svgimg/png = user artwork; appicon = extracted application icon; brandicon = untouched official built-in artwork
    pub kind: String,
    /// data: URL for svgimg/png/appicon
    pub url: String,
    /// Trusted built-in SVG text (script and on* event attributes removed defensively)
    pub svg: String,
    /// Where it came from (for doctor)
    pub source: String,
}

pub const IDS: [&str; 5] = ["claude", "codex", "cursor", "copilot", "gemini"];

/// Minimal SVG sanitising before inlining into the DOM: drop <script> blocks and on*="…" event
/// attributes (the built-in files have none; this guards user files). Every slice position comes
/// from an ASCII pattern match and lands on a character boundary, so non-ASCII content is safe.
fn sanitize_svg(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let lower = s.to_ascii_lowercase();
    let mut i = 0;
    while let Some(rel) = lower[i..].find("<script") {
        out.push_str(&s[i..i + rel]);
        match lower[i + rel..].find("</script>") {
            Some(e) => i = i + rel + e + "</script>".len(),
            None => {
                i = s.len();
                break;
            }
        }
    }
    out.push_str(&s[i..]);
    let lo = out.to_ascii_lowercase();
    let mut res = String::with_capacity(out.len());
    let mut i = 0;
    loop {
        let Some(rel) = lo[i..].find(" on") else {
            break;
        };
        let start = i + rel;
        let name_len = lo[start + 3..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric())
            .count();
        let eq = start + 3 + name_len;
        if name_len > 0 && lo.as_bytes().get(eq) == Some(&b'=') {
            if let Some(&q) = lo.as_bytes().get(eq + 1) {
                if q == b'"' || q == b'\'' {
                    if let Some(close) = lo[eq + 2..].find(q as char) {
                        res.push_str(&out[i..start]);
                        i = eq + 2 + close + 1;
                        continue;
                    }
                }
            }
        }
        res.push_str(&out[i..start + 3]);
        i = start + 3;
    }
    res.push_str(&out[i..]);
    res
}

fn glyph_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = crate::config::config_path().parent() {
        v.push(d.join("glyphs"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("glyphs"));
        }
    }
    v
}

pub fn user_dir() -> PathBuf {
    crate::config::config_path().with_file_name("glyphs")
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn built_in(id: &str) -> Option<Glyph> {
    match id {
        "claude" => Some(Glyph {
            kind: "brandicon".into(),
            svg: sanitize_svg(include_str!("../glyphs/claude-original.svg")),
            source: "built-in · Anthropic official press kit · ClaudeIcon-Rounded.svg".into(),
            ..Default::default()
        }),
        "codex" => Some(Glyph {
            kind: "brandicon".into(),
            url: format!(
                "data:image/png;base64,{}",
                b64(include_bytes!("../glyphs/codex-original.png"))
            ),
            source: "built-in · official Codex for Windows package · assets/icon.png".into(),
            ..Default::default()
        }),
        "cursor" => Some(Glyph {
            kind: "brandicon".into(),
            url: format!(
                "data:image/png;base64,{}",
                b64(include_bytes!("../glyphs/cursor-original.png"))
            ),
            source: "built-in · Cursor official brand kit · APP_ICON_25D_DARK.png".into(),
            ..Default::default()
        }),
        "copilot" => Some(Glyph {
            kind: "brandicon".into(),
            svg: sanitize_svg(include_str!("../glyphs/github-original.svg")),
            source: "built-in · official GitHub brand kit · GitHub_Invertocat_White_Clearspace.svg"
                .into(),
            ..Default::default()
        }),
        "gemini" => Some(Glyph {
            kind: "svg".into(),
            svg: sanitize_svg(include_str!("../glyphs/gemini.svg")),
            source: "built-in · @lobehub/icons-static-svg 1.95.0 (MIT)".into(),
            ..Default::default()
        }),
        _ => None,
    }
}

fn from_file(p: &Path) -> Option<Glyph> {
    let bytes = std::fs::read(p).ok()?;
    if bytes.is_empty() || bytes.len() > 512 * 1024 {
        return None;
    }
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "svg" => Some(Glyph {
            // Never inline user-controlled SVG markup in the WebView. Loading it as an image
            // preserves custom artwork while keeping its document and script context inert.
            kind: "svgimg".into(),
            url: format!("data:image/svg+xml;base64,{}", b64(&bytes)),
            source: p.display().to_string(),
            ..Default::default()
        }),
        "png" => Some(Glyph {
            kind: "png".into(),
            url: format!("data:image/png;base64,{}", b64(&bytes)),
            source: p.display().to_string(),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Candidate executables of the installed apps (Windows); MSIX store versions live under WindowsApps where a normal process cannot read them, and that is fine
fn app_candidates(id: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let Some(local) = dirs::data_local_dir() else {
        return v;
    };
    let programs = local.join("Programs");
    match id {
        "claude" => {
            v.push(local.join("AnthropicClaude").join("claude.exe"));
            if let Ok(rd) = std::fs::read_dir(local.join("AnthropicClaude")) {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().starts_with("app-") {
                        v.push(e.path().join("claude.exe"));
                    }
                }
            }
            v.push(programs.join("Claude").join("Claude.exe"));
            v.push(programs.join("claude-desktop").join("Claude.exe"));
        }
        "codex" => {
            v.push(programs.join("ChatGPT").join("ChatGPT.exe"));
            v.push(programs.join("Codex").join("Codex.exe"));
            if let Some(exe) = crate::codex::find_executable() {
                v.push(exe);
            }
        }
        "cursor" => {
            v.push(programs.join("cursor").join("Cursor.exe"));
            v.push(programs.join("Cursor").join("Cursor.exe"));
        }
        "copilot" => {
            v.push(programs.join("GitHub Copilot").join("GitHubCopilot.exe"));
            v.push(programs.join("GitHub Copilot").join("copilot.exe"));
            if let Some(exe) = crate::copilot::find_executable() {
                v.push(exe);
            }
        }
        "gemini" => {
            v.push(programs.join("Antigravity").join("Antigravity.exe"));
            v.push(programs.join("antigravity").join("Antigravity.exe"));
        }
        _ => {}
    }
    v.extend(running_app_candidates(id));
    v.into_iter().filter(|p| p.is_file()).collect()
}

/// Resolve the executable paths of already-running provider apps. This is important for MSIX apps
/// such as Codex: their versioned WindowsApps path cannot be guessed or enumerated reliably, but a
/// process may disclose its own image path through PROCESS_QUERY_LIMITED_INFORMATION.
#[cfg(windows)]
fn running_app_candidates(id: &str) -> Vec<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let names: &[&str] = match id {
        "claude" => &["claude.exe", "claude desktop.exe"],
        "codex" => &["chatgpt.exe", "codex.exe"],
        "cursor" => &["cursor.exe"],
        "copilot" => &["githubcopilot.exe", "github copilot.exe", "copilot.exe"],
        "gemini" => &["antigravity.exe"],
        _ => &[],
    };
    let maps = crate::focus::proc_maps();
    let mut paths = Vec::new();
    for (pid, name) in maps.name {
        if !names.iter().any(|candidate| *candidate == name) {
            continue;
        }
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                continue;
            };
            let mut buf = vec![0u16; 32_768];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                paths.push(PathBuf::from(String::from_utf16_lossy(
                    &buf[..len as usize],
                )));
            }
            let _ = CloseHandle(handle);
        }
    }
    paths
}

#[cfg(not(windows))]
fn running_app_candidates(_id: &str) -> Vec<PathBuf> {
    Vec::new()
}

/// Icon from the exe resources → 64 px RGBA → PNG data URL
#[cfg(windows)]
fn from_exe(p: &Path) -> Option<Glyph> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = p.as_os_str().encode_wide().collect();
    if wide.len() >= 260 {
        return None;
    }
    let mut name = [0u16; 260];
    name[..wide.len()].copy_from_slice(&wide);
    const SIZE: i32 = 64;
    unsafe {
        let mut icons = [HICON::default(); 1];
        let mut id = 0u32;
        let n = PrivateExtractIconsW(
            &name,
            0,
            SIZE,
            SIZE,
            Some(&mut icons[..]),
            Some(&mut id as *mut u32),
            0,
        );
        if n == 0 || icons[0].is_invalid() {
            return None;
        }
        let hicon = icons[0];
        let mut info = ICONINFO::default();
        let ok = GetIconInfo(hicon, &mut info).is_ok();
        let mut result = None;
        if ok && !info.hbmColor.is_invalid() {
            let mut bm = BITMAP::default();
            GetObjectW(
                info.hbmColor,
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut _),
            );
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if w > 0 && h > 0 && w <= 512 && h <= 512 {
                let hdc = GetDC(None);
                let mut bi = BITMAPINFO::default();
                bi.bmiHeader = BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                };
                let mut buf = vec![0u8; (w * h * 4) as usize];
                let lines = GetDIBits(
                    hdc,
                    info.hbmColor,
                    0,
                    h as u32,
                    Some(buf.as_mut_ptr() as *mut _),
                    &mut bi,
                    DIB_RGB_COLORS,
                );
                let _ = ReleaseDC(None, hdc);
                if lines > 0 {
                    // BGRA → RGBA; old-style icons with all-zero alpha are treated as opaque
                    let any_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
                    for px in buf.chunks_exact_mut(4) {
                        px.swap(0, 2);
                        if !any_alpha {
                            px[3] = 255;
                        }
                    }
                    result = encode_png(w as u32, h as u32, &buf).map(|png| Glyph {
                        kind: "appicon".into(),
                        url: format!("data:image/png;base64,{}", b64(&png)),
                        source: p.display().to_string(),
                        ..Default::default()
                    });
                }
            }
        }
        if ok {
            let _ = DeleteObject(info.hbmColor);
            let _ = DeleteObject(info.hbmMask);
        }
        let _ = DestroyIcon(hicon);
        result
    }
}
#[cfg(not(windows))]
fn from_exe(_p: &Path) -> Option<Glyph> {
    None
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(rgba).ok()?;
        wr.finish().ok()?;
    }
    Some(out)
}

/// Collects every provider glyph (once at startup, again on the tray's refresh)
pub fn collect() -> HashMap<String, Glyph> {
    let mut map = HashMap::new();
    let dirs = glyph_dirs();
    for id in IDS {
        let mut found: Option<Glyph> = None;
        'dirs: for d in &dirs {
            for ext in ["svg", "png"] {
                let p = d.join(format!("{id}.{ext}"));
                if let Some(g) = from_file(&p) {
                    found = Some(g);
                    break 'dirs;
                }
            }
        }
        if found.is_none() {
            found = built_in(id);
        }
        // Keep the packaged app pixel-identical to the HTML preview. Executable resources vary by
        // installation channel and version (some are monochrome or padded differently), so they
        // are a final fallback rather than silently replacing the curated brand artwork.
        if found.is_none() {
            for exe in app_candidates(id) {
                if let Some(g) = from_exe(&exe) {
                    found = Some(g);
                    break;
                }
            }
        }
        if let Some(g) = found {
            map.insert(id.to_string(), g);
        }
    }
    map
}

/// For doctor
pub fn probe() -> String {
    let m = collect();
    let mut lines = vec![format!(
        "glyph directory: {} (drop claude/codex/cursor/copilot/gemini .svg or .png files here)",
        user_dir().display()
    )];
    for id in IDS {
        lines.push(match m.get(id) {
            Some(g) => format!("  {id}: {} ← {}", g.kind, g.source),
            None => format!("  {id}: fallback letter (built-in artwork missing?)"),
        });
    }
    lines.join("\n")
}
