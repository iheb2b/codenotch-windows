use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How small the notch may be drawn, as a multiple of its designed size. Below roughly 0.4 the
/// rings stop being readable at 100 % display scaling.
pub const SCALE_MIN: f64 = 0.40;
pub const SCALE_MAX: f64 = 1.00;

/// One half of the tray icon: which provider, and which of its windows.
/// `window` is a window id as the provider reports it ("session", "weekly_all", "primary"…), or
/// the empty string / "top" meaning "whichever of its windows is fullest" — the same rule the
/// notch ring uses, and the only choice that keeps working when a provider changes its windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraySlot {
    pub provider: String,
    #[serde(default)]
    pub window: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    /// Shared only between the local hook helper and the loopback event server. The custom header
    /// prevents an arbitrary web page from manufacturing an approval card through a no-CORS POST.
    #[serde(default)]
    pub bridge_token: String,
    /// "auto" | "zh" | "en" | "ja" | "ko" | "ru"
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default)]
    pub bar_x: Option<i32>,
    #[serde(default)]
    pub bar_y: Option<i32>,
    /// Logical width of the bar (wheel-adjustable, 220-520); None = default 360
    #[serde(default)]
    pub bar_w: Option<u32>,
    /// Allow dragging + wheel resizing (tray toggle, off by default to prevent accidental drags)
    #[serde(default)]
    pub drag_enabled: bool,
    /// Vertical position of the notch: the window centre as a fraction of the primary monitor's height (0 = top, 1 = bottom), default 0.5; saved after a drag
    #[serde(default = "default_notch_y")]
    pub notch_y: f64,
    /// Notch size as a multiple of the designed size (slider at the foot of the hover card).
    /// Only the pill is scaled — the hover card keeps its size, so the slider does not move
    /// while it is being dragged.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// What the tray icon draws: "off" (the plain mark, the previous behaviour and the default),
    /// "numbers" (up to two readings as digits) or "bars" (a column per reading).
    #[serde(default = "default_tray_mode")]
    pub tray_mode: String,
    /// Which providers the tray icon covers, in the order they are drawn. Ids match the page:
    /// "claude", "codex", "cursor", "copilot", "gemini". Superseded by `tray_slots`; kept so an existing
    /// config still upgrades cleanly, and migrated in `load()`.
    #[serde(default = "default_tray_providers")]
    pub tray_providers: Vec<String>,
    /// What each part of the tray icon shows, in drawing order: the first entry is the top half of
    /// the digit layout, the second the bottom half, and the bar layout uses them all in order.
    #[serde(default)]
    pub tray_slots: Vec<TraySlot>,
    /// Which providers the notch itself shows, in order. Empty means every provider that has
    /// something to report — the original behaviour, and the default. Superseded by `notch_slots`,
    /// kept so an existing config migrates cleanly.
    #[serde(default)]
    pub notch_providers: Vec<String>,
    /// What each ring on the notch shows: the provider, and which of its windows. An empty list
    /// means every provider, each showing whichever of its windows is fullest — the original
    /// behaviour. Same shape as the tray slots so the two settings read alike.
    #[serde(default)]
    pub notch_slots: Vec<TraySlot>,
    /// Which providers the edge capsule shows: "smart" keeps only open/working tools (and a
    /// single useful fallback), "pinned" follows `notch_slots`, and "all" shows every available
    /// provider. Smart is deliberately the default so old usage history does not look like a
    /// currently open application.
    #[serde(default = "default_notch_mode")]
    pub notch_mode: String,
    /// false = the pill is kept off the screen edge entirely; the tray icon is then the only way in
    #[serde(default = "yes")]
    pub notch_visible: bool,
    /// false = the tray icon is hidden. Refused while the notch is also hidden, because that would
    /// leave the app running with no way to reach it.
    #[serde(default = "yes")]
    pub tray_visible: bool,
}

fn default_notch_y() -> f64 {
    0.5
}
fn default_scale() -> f64 {
    1.0
}
fn default_notch_mode() -> String {
    "smart".into()
}
fn yes() -> bool {
    true
}
/// A fresh install shows the two readings straight away — a tray icon nobody knows to look for is
/// a feature nobody finds. An install that predates this setting is handled in `load()` instead:
/// it keeps the plain mark it already has, so upgrading never changes anyone's icon unasked.
fn default_tray_mode() -> String {
    "numbers".into()
}
fn default_tray_providers() -> Vec<String> {
    vec!["claude".into(), "codex".into()]
}

fn default_port() -> u16 {
    48666
}
fn default_lang() -> String {
    "auto".into()
}

fn valid_bridge_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: default_port(),
            bridge_token: String::new(),
            lang: default_lang(),
            bar_x: None,
            bar_y: None,
            bar_w: None,
            drag_enabled: false,
            notch_y: default_notch_y(),
            scale: default_scale(),
            tray_mode: default_tray_mode(),
            tray_providers: default_tray_providers(),
            tray_slots: Vec::new(), // filled in by load(), from tray_providers
            notch_providers: Vec::new(), // empty = show them all
            notch_slots: Vec::new(), // filled in by load(), from notch_providers
            notch_mode: default_notch_mode(),
            notch_visible: true,
            tray_visible: true,
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("codenotch")
        .join("config.json")
}

pub fn load() -> Config {
    let path = config_path();
    let raw = std::fs::read_to_string(&path).ok();
    let parsed = raw.as_deref().map(serde_json::from_str::<Config>);
    let mut cfg: Config = match parsed {
        Some(Ok(cfg)) => cfg,
        Some(Err(error)) => {
            // Startup persists migrations immediately. Preserve a malformed hand-edited or
            // partially-written file before defaults replace it, so recovery remains possible.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let backup = path.with_file_name(format!("config.corrupt-{stamp}.json"));
            let _ = std::fs::copy(&path, &backup);
            eprintln!(
                "invalid config {} ({error}); backup: {}",
                path.display(),
                backup.display()
            );
            Config::default()
        }
        None => Config::default(),
    };

    if !valid_bridge_token(&cfg.bridge_token) {
        // A random browser-to-localhost CSRF boundary, not an account credential. Processes
        // running as the same Windows user can already read the provider settings.
        cfg.bridge_token = uuid::Uuid::new_v4().simple().to_string();
    }

    // Discoverability without surprising anyone. `default_tray_mode` gives a NEW install the
    // numbers icon, but serde applies that same default to an EXISTING config that simply predates
    // the setting — which would silently change the tray icon of everyone who upgrades. So an
    // existing file with no `tray_mode` key is pinned to the plain mark it already has; only a
    // machine with no config at all gets the new default.
    let upgrading = raw
        .as_deref()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .map(|v| v.get("tray_mode").is_none())
        .unwrap_or(false);
    if upgrading {
        cfg.tray_mode = "off".into();
    }

    // Migration: before slots existed the icon was a plain provider list, each showing whichever of
    // its windows was fullest. That is exactly a slot with an empty `window`, so nobody's choice is
    // lost and nobody has to reconfigure anything.
    if cfg.tray_slots.is_empty() {
        cfg.tray_slots = cfg
            .tray_providers
            .iter()
            .map(|p| TraySlot {
                provider: p.clone(),
                window: String::new(),
            })
            .collect();
    }

    // Same migration for the notch: a plain provider list becomes slots that each show whichever
    // window is fullest, which is exactly what the list used to mean.
    if cfg.notch_slots.is_empty() {
        cfg.notch_slots = cfg
            .notch_providers
            .iter()
            .map(|p| TraySlot {
                provider: p.clone(),
                window: String::new(),
            })
            .collect();
    }

    // Both hidden would leave the app unreachable: no pill, no tray icon, no way to open settings.
    if !cfg.notch_visible && !cfg.tray_visible {
        cfg.tray_visible = true;
    }

    // A hand-edited file must not be able to produce an invisible window
    cfg.scale = cfg.scale.clamp(SCALE_MIN, SCALE_MAX);
    if !matches!(cfg.notch_mode.as_str(), "smart" | "pinned" | "all") {
        cfg.notch_mode = default_notch_mode();
    }
    cfg
}

/// Replace a small settings file through a fully-written sibling temporary file. Keeping the
/// temporary file in the same directory makes the final rename a same-volume atomic operation.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("settings");
    let temp = path.with_file_name(format!("{name}.tmp.{}.{}", std::process::id(), id));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn save(cfg: &Config) {
    let path = config_path();
    if let Ok(txt) = serde_json::to_string_pretty(cfg) {
        if let Err(error) = atomic_write(&path, txt.as_bytes()) {
            eprintln!("cannot save {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::atomic_write;

    #[test]
    fn atomic_write_replaces_complete_content() {
        let dir = std::env::temp_dir().join(format!("codenotch-config-{}", std::process::id()));
        let path = dir.join("config.json");
        let _ = std::fs::create_dir_all(&dir);
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!dir
            .read_dir()
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".tmp.")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
