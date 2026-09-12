# Codenotch for Windows

[![Build Windows app](https://github.com/iheb2b/codenotch-windows/actions/workflows/windows-release.yml/badge.svg)](https://github.com/iheb2b/codenotch-windows/actions/workflows/windows-release.yml)
[![Latest release](https://img.shields.io/github/v/release/iheb2b/codenotch-windows?display_name=tag&sort=semver)](https://github.com/iheb2b/codenotch-windows/releases/latest)

[![Download Codenotch for Windows](https://img.shields.io/badge/Download-Codenotch%20for%20Windows-0078D4?style=for-the-badge&logo=windows11&logoColor=white)](https://github.com/iheb2b/codenotch-windows/releases/latest/download/Codenotch.exe)

**Windows 10/11 · x64 · Portable EXE**

[Release notes](https://github.com/iheb2b/codenotch-windows/releases/latest) · [SHA-256 checksums](https://github.com/iheb2b/codenotch-windows/releases/latest/download/SHA256SUMS.txt)

A Windows port of [Codenotch](https://github.com/vinzdg/codenotch) — the usage notch that
sits on the edge of your screen and answers three questions at a glance:
**what needs me**, **what is still working**, and **how much AI allowance is left**.

The compact rail uses a quiet, translucent Windows surface with real installed-app icons and a
progressive-disclosure hover card. A separate dashboard holds the detailed work queue, usage pace,
GitHub build status, and privacy information without crowding the screen edge.
No code is copied from the Swift app; the providers are reimplemented from their
documented behaviour and the wire formats.

## What it shows

| Cell | Source | How it reads it |
|---|---|---|
| **Claude** | `GET https://api.anthropic.com/api/oauth/usage` with the token Claude Code keeps in `~/.claude/.credentials.json` | Session / weekly windows, 429 back-off with a persisted deadline, stale readings dimmed with their age. The rail reports live work or review state without continuous animation (Claude Code hooks + transcript watcher, desktop app included). |
| **Codex** | `GET https://chatgpt.com/backend-api/wham/usage` with the session Codex keeps in `~/.codex/auth.json` (read only, never refreshed), falling back to the `rate_limits` snapshot in the newest rollout log | Live primary/secondary windows (5h + weekly on paid plans, a monthly window on free) while Codex is signed in; otherwise the last snapshot, marked stale by its own timestamp. |
| **Cursor** | The editor's own session from `state.vscdb` → `cursor.com/api/usage-summary` | Included usage / API usage / on-demand, reset at billing-cycle end. Nothing to sign into: it borrows the editor's session, so there is only ever one account. |
| **GitHub Copilot** | Official GitHub Copilot SDK's experimental `account.getQuota` RPC through the user's existing signed-in Copilot CLI | Premium requests / AI credits and reset date when the account publishes a finite quota. The CLI is not bundled, no Copilot session or prompt is created, credentials remain inside Copilot, and an incompatible CLI change degrades to an unavailable reading. |
| **Antigravity** | Official `agy` CLI `/usage` print when installed; otherwise the existing local `language_server` bridge, Google Cloud Code API, or transcript model count | Official four quota rows (Gemini & Claude/GPT 5h/weekly) without running the full IDE. When CLI is absent, falls back to legacy local bridge/API. |

Providers that are not installed simply do not get a cell.

## Work dashboard

- **Attention inbox** — waiting work is sorted first. Claude Code can expose the tool and an action preview through its documented `PermissionRequest` hook, enabling one-time Approve/Deny directly in the notch or release to Claude's full review. Codex, Cursor, and Copilot keep a safe **Review in app** action unless Codenotch owns their session; it never synthesizes clicks.
- **Usage runway** — records only timestamped usage percentages in WebView local storage and learns a 30-day pace estimate. It never stores prompts or credentials in history.
- **Build watch** — reads the latest GitHub Actions run and release for a public `owner/repository`. Monitoring is read-only: it cannot rerun, cancel, or edit workflows.
- **Privacy view** — names the local files and remote services behind each reading.

The dashboard is available from the edge card and the tray menu. It remains useful with no GitHub repository configured.

Direct Claude Code approvals are opt-in. Open **Settings → Approvals**, enable the Claude Code
bridge, and restart any already-running Claude Code session so it reloads its hooks. Codenotch
backs up `.claude\settings.json` before merging its entries and leaves unrelated hooks intact.

### Antigravity

- **Official CLI (Preferred)**: When the official Antigravity CLI (`agy.exe`) is installed (`%LOCALAPPDATA%\agy\bin\agy.exe` or on `PATH`) and signed in, Codenotch reads official quotas directly without keeping the full IDE running.
- **Execution**: Runs the official CLI in a hidden Windows pseudo-console, with a 70-second timeout and cleanup of its process tree. It does not need PowerShell scripts or a separate service.
- **Refresh**: Checks at startup and on hover/explicit request when readings are at least five minutes old; failed attempts are also limited to once per five minutes. It keeps previous readings on failure, without switching to legacy APIs. The CLI is not launched periodically while idle.
- **Fallback**: When the official CLI is not installed, Codenotch preserves the legacy local bridge (`language_server`), Credential Manager, and transcript model turn counting to maintain compatibility with existing installations.
- **Official CLI Reference**: Standalone `/usage` printing is described in the [official Antigravity CLI documentation](https://www.antigravity.google/docs/cli/headless). Note: no categorical Terms of Service guarantee is made.

Restart Codenotch after installing or removing `agy`: the source is selected at startup.
The CLI's text report is parsed defensively; an unsupported format or failed sign-in
shows an error or the last reading marked stale. Codenotch does not automate sign-in.

## Install / build

Prerequisites: Rust 1.94 or newer (MSVC toolchain), WebView2 runtime (ships with Windows 11).

```powershell
# from the repository root
cargo build --release
.\target\release\codenotch.exe          # pill appears on the right edge of the primary monitor
.\target\release\codenotch.exe doctor   # self-diagnosis: credentials, data sources, icons, hooks
```

Tray menu: dashboard, settings, refresh now, and quit. Detailed controls live in Settings; logs,
persisted readings, and icon overrides live in `%APPDATA%\codenotch`.

## Download a Windows build

Click **Download Codenotch for Windows** at the top of this page. The button always downloads the
standalone `Codenotch.exe` from the latest stable GitHub release, so it does not need to change when
a new version is published. Run the downloaded executable directly; no ZIP extraction is required.

Every release also includes the standalone executables, release notes, and SHA-256 checksums on the
[Releases page](https://github.com/iheb2b/codenotch-windows/releases/latest).

### Smart App Control and Windhawk

If Windows says **“Smart App Control has blocked part of this app”**, first inspect
**Event Viewer → Applications and Services Logs → Microsoft → Windows → CodeIntegrity → Operational**.
That message identifies a secondary file another program tried to load; it does not necessarily mean
that Codenotch itself was blocked.

Windhawk injects its engine into processes by default. If the event names
`Program Files\Windhawk\...\windhawk.dll`, keep Smart App Control enabled and add these entries to
**Windhawk → Settings → Advanced settings → More advanced settings → Process exclusion list**:

```text
Codenotch.exe
msedgewebview2.exe
```

Restart Windhawk and Codenotch afterward. Excluded processes are unaffected by Windhawk, while the
rest of your Windhawk customizations continue to work. Codenotch's own `doctor` command reports this
compatibility condition when Windhawk is installed.

### Icons

Claude and Cursor use the unmodified app icons from their official press/brand kits. Codex uses
the icon shipped in OpenAI's signed Windows package, and Copilot uses GitHub's official Invertocat
as its provider identifier. Antigravity retains its MIT-licensed fallback
mark. Exact provenance and SHA-256 hashes are recorded in `codenotch/glyphs/NOTICE.md`. Drop your
own `claude|codex|cursor|copilot|gemini.svg` (or `.png`) into `%APPDATA%\codenotch\glyphs\` to override.
All provider marks remain trademarks of their respective owners.

## Layout

```
.
├── codenotch/          Tauri 2 app: window, tray, providers (usage.rs, codex.rs, cursor.rs, copilot.rs, antigravity.rs),
│   ├── src/            session engine, ephemeral approvals, glyphs, focus, and diagnostics
│   ├── ui/notch.html   the pill + hover card (single file, no framework)
│   ├── ui/dashboard.html attention, usage runway, build watch, and privacy views
│   └── glyphs/         provider marks (+ NOTICE.md)
└── codenotch-hook/     fast activity messenger; only PermissionRequest waits for a one-shot answer
```

## Relationship to upstream

This port follows the upstream design spec (`docs/specs/2026-08-28-usage-notch-design.md`)
and provider semantics. It is developed at
[iheb2b/codenotch-windows](https://github.com/iheb2b/codenotch-windows) and offered to the
upstream project as its `windows/` tree; the two are kept in sync. The session-detection engine
originated in [Im-Midi/Pac-Man](https://github.com/Im-Midi/Pac-Man) (MIT).

## License

MIT — see `LICENSE`. The Codenotch design and name belong to the upstream author.
