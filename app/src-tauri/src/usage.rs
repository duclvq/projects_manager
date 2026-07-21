use crate::discovery::default_sources;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// One authoritative Claude plan window as reported by the `/usage` command.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClaudeLimit {
    pub used_percent: f64,
    pub reset_label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClaudeUsage {
    pub session: ClaudeLimit,
    pub weekly: ClaudeLimit,
    /// True when the latest fetch failed and this is the last successful snapshot.
    pub stale: bool,
}

static CLAUDE_CACHE: OnceLock<Mutex<Option<ClaudeUsage>>> = OnceLock::new();

// Claude Code exposes subscription limits only through its authenticated `/usage`
// surface. Expect gives the command the TTY it requires without sending a model
// prompt. The process is short-lived and runs outside every managed project.
const CLAUDE_USAGE_EXPECT: &str = r#"
log_user 1
set timeout 15
set claude_bin $env(PROJECTS_MANAGER_CLAUDE_BIN)
set env(TERM) "xterm-256color"
spawn -noecho $claude_bin --no-chrome
after 1500
send -- "/usage\r"
expect {
  -re {What.s contributing|Esc to cancel} {}
  -re {Failed.*usage data} { exit 4 }
  timeout { exit 2 }
  eof { exit 3 }
}
after 300
send -- "\033"
after 200
send -- "/exit\r"
expect {
  eof {}
  timeout { send -- "\003"; expect eof }
}
"#;

/// Codex rate-limit snapshot, read from the most recent `token_count` event.
#[derive(Debug, Clone, Serialize)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodexUsage {
    pub primary: RateLimitWindow,
    pub secondary: Option<RateLimitWindow>,
    pub plan_type: Option<String>,
    pub context_remaining_percent: Option<f64>,
    pub context_used_tokens: Option<u64>,
    pub context_window_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub claude: Option<ClaudeUsage>,
    pub codex: Option<CodexUsage>,
}

pub fn build() -> Usage {
    let sources = default_sources();
    Usage {
        claude: claude_usage(),
        codex: codex_usage(&sources.codex_root),
    }
}

fn claude_usage() -> Option<ClaudeUsage> {
    let cache = CLAUDE_CACHE.get_or_init(|| Mutex::new(None));
    match fetch_claude_usage() {
        Ok(usage) => {
            if let Ok(mut saved) = cache.lock() {
                *saved = Some(usage.clone());
            }
            Some(usage)
        }
        Err(_) => cache.lock().ok().and_then(|saved| {
            saved.clone().map(|mut usage| {
                usage.stale = true;
                usage
            })
        }),
    }
}

fn fetch_claude_usage() -> Result<ClaudeUsage, String> {
    let claude = find_claude_binary().ok_or_else(|| "claude executable not found".to_string())?;
    let output = Command::new("/usr/bin/expect")
        .arg("-c")
        .arg(CLAUDE_USAGE_EXPECT)
        .current_dir(std::env::temp_dir())
        .env("PROJECTS_MANAGER_CLAUDE_BIN", claude)
        .env("DISABLE_AUTOUPDATER", "1")
        .env("DISABLE_TELEMETRY", "1")
        .output()
        .map_err(|e| format!("failed to run Claude /usage: {e}"))?;
    if !output.status.success() {
        return Err(format!("Claude /usage exited with {}", output.status));
    }
    parse_claude_usage_output(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Claude /usage output did not contain both limits".to_string())
}

fn find_claude_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_BIN").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for relative in [".local/bin/claude", ".claude/local/claude"] {
            let path = home.join(relative);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    let output = Command::new("/bin/zsh")
        .args(["-lc", "command -v claude"])
        .output()
        .ok()?;
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    path.is_file().then_some(path)
}

fn parse_claude_usage_output(raw: &str) -> Option<ClaudeUsage> {
    let text = terminal_text(raw);
    let session_start = text.find("Current session")?;
    let week_offset = text[session_start..].find("Current week")?;
    let week_start = session_start + week_offset;
    let week_end = text[week_start..]
        .find("What's contributing")
        .map(|offset| week_start + offset)
        .unwrap_or(text.len());
    Some(ClaudeUsage {
        session: parse_claude_limit(&text[session_start..week_start])?,
        weekly: parse_claude_limit(&text[week_start..week_end])?,
        stale: false,
    })
}

fn parse_claude_limit(section: &str) -> Option<ClaudeLimit> {
    let percent = section.find('%')?;
    let used_text = section[..percent].trim_end();
    let number_start = used_text
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|index| index + 1)
        .unwrap_or(0);
    let used_percent = used_text[number_start..].parse::<f64>().ok()?;
    let reset = section.find("Resets")? + "Resets".len();
    let reset_line = section[reset..].lines().next().unwrap_or("");
    Some(ClaudeLimit {
        used_percent,
        reset_label: reset_line.split_whitespace().collect::<Vec<_>>().join(" "),
    })
}

/// Convert terminal cursor/colour controls to whitespace while preserving text
/// and line boundaries, making the TUI output deterministic enough to parse.
fn terminal_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for code in chars.by_ref() {
                        if ('@'..='~').contains(&code) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for code in chars.by_ref() {
                        if code == '\x07' {
                            break;
                        }
                    }
                }
                Some(_) | None => {}
            }
            out.push(' ');
        } else if c == '\r' || c == '\n' {
            out.push('\n');
        } else if c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn codex_rollouts(root: &Path, out: &mut Vec<(SystemTime, std::path::PathBuf)>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            codex_rollouts(&p, out);
        } else if p.extension().map_or(false, |x| x == "jsonl") {
            if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) {
                out.push((m, p));
            }
        }
    }
}

fn codex_usage(root: &Path) -> Option<CodexUsage> {
    let mut files = Vec::new();
    codex_rollouts(root, &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0)); // newest first

    // Walk newest files until we find the most recent rate_limits snapshot.
    for (_, path) in files.iter().take(20) {
        let Ok(content) = fs::read_to_string(path) else { continue };
        let mut latest: Option<CodexUsage> = None;
        let mut context: Option<(f64, u64, u64)> = None;
        for line in content.lines() {
            if !line.contains("\"rate_limits\"") && !line.contains("\"model_context_window\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if let Some(info) = v.pointer("/payload/info") {
                context = parse_codex_context(info);
            }
            if let Some(rl) = v.pointer("/payload/rate_limits") {
                latest = parse_codex_rate_limits(rl);
            }
        }
        if let Some(mut usage) = latest {
            if let Some((remaining, used, window)) = context {
                usage.context_remaining_percent = Some(remaining);
                usage.context_used_tokens = Some(used);
                usage.context_window_tokens = Some(window);
            }
            return Some(usage);
        }
    }
    None
}

fn parse_rate_limit_window(value: &Value) -> Option<RateLimitWindow> {
    Some(RateLimitWindow {
        used_percent: value.get("used_percent")?.as_f64()?,
        window_minutes: value.get("window_minutes")?.as_u64()?,
        resets_at: value.get("resets_at").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

fn parse_codex_rate_limits(value: &Value) -> Option<CodexUsage> {
    Some(CodexUsage {
        primary: parse_rate_limit_window(value.get("primary")?)?,
        secondary: value.get("secondary").and_then(parse_rate_limit_window),
        plan_type: value.get("plan_type").and_then(|x| x.as_str()).map(String::from),
        context_remaining_percent: None,
        context_used_tokens: None,
        context_window_tokens: None,
    })
}

fn parse_codex_context(value: &Value) -> Option<(f64, u64, u64)> {
    let used = value.pointer("/last_token_usage/total_tokens")?.as_u64()?;
    let window = value.get("model_context_window")?.as_u64()?;
    if window == 0 {
        return None;
    }
    let remaining = (100.0 * (1.0 - used as f64 / window as f64)).clamp(0.0, 100.0);
    Some((remaining, used, window))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_authoritative_claude_usage_from_tui_output() {
        let output = "\x1b[3CCurrent\x1b[12Gsession\r\n\
            \x1b[55G100%\x1b[60Gused\r\n\
            \x1b[3CResets\x1b[11G5:49pm\x1b[18G(Asia/Saigon)\r\n\
            \x1b[3CCurrent\x1b[12Gweek\x1b[17G(all models)\r\n\
            \x1b[55G24%\x1b[59Gused\r\n\
            \x1b[3CResets\x1b[11GJul\x1b[15G25\x1b[18Gat\x1b[21G5:59am\x1b[28G(Asia/Saigon)\r\n\
            What's contributing to your limits usage?";
        let usage = parse_claude_usage_output(output).unwrap();
        assert_eq!(usage.session.used_percent, 100.0);
        assert_eq!(usage.session.reset_label, "5:49pm (Asia/Saigon)");
        assert_eq!(usage.weekly.used_percent, 24.0);
        assert_eq!(usage.weekly.reset_label, "Jul 25 at 5:59am (Asia/Saigon)");
        assert!(!usage.stale);
    }

    #[test]
    fn rejects_incomplete_claude_usage_output() {
        assert!(parse_claude_usage_output("Current session 10% used").is_none());
    }

    #[test]
    fn parses_both_codex_rate_limit_windows() {
        let value = serde_json::json!({
            "primary": {"used_percent": 21.0, "window_minutes": 300, "resets_at": 100},
            "secondary": {"used_percent": 37.0, "window_minutes": 10080, "resets_at": 200},
            "plan_type": "plus"
        });
        let usage = parse_codex_rate_limits(&value).unwrap();
        assert_eq!(usage.primary.window_minutes, 300);
        assert_eq!(usage.secondary.unwrap().window_minutes, 10080);
        assert_eq!(usage.plan_type.as_deref(), Some("plus"));
    }

    #[test]
    fn accepts_codex_snapshot_without_secondary_window() {
        let value = serde_json::json!({
            "primary": {"used_percent": 13.0, "window_minutes": 10080, "resets_at": 300},
            "secondary": null
        });
        let usage = parse_codex_rate_limits(&value).unwrap();
        assert!(usage.secondary.is_none());
    }

    #[test]
    fn calculates_codex_context_remaining_from_last_request() {
        let value = serde_json::json!({
            "last_token_usage": {"total_tokens": 79_000},
            "model_context_window": 258_000
        });
        let (remaining, used, window) = parse_codex_context(&value).unwrap();
        assert!((remaining - 69.38).abs() < 0.01);
        assert_eq!(used, 79_000);
        assert_eq!(window, 258_000);
    }
}
