use crate::discovery::default_sources;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Claude token usage rolled up over rolling windows (computed from the per-message
/// `usage` fields in the session JSONL — Claude does not expose a plan limit locally,
/// so this is spend, not "remaining").
/// Estimated weekly "billable" token budget for the remaining-% gauge. Claude
/// does not expose a real limit locally, so this is an editable estimate — set it
/// to whatever your plan's weekly ceiling feels like (billable = input + output +
/// cache-writes; cache-reads are excluded because they barely count toward limits).
const CLAUDE_WEEKLY_BUDGET: u64 = 40_000_000;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClaudeUsage {
    pub today_tokens: u64,
    pub today_cost: f64,
    pub week_tokens: u64,
    pub week_cost: f64,
    /// Billable tokens this week (input + output + cache-writes; excludes cache-reads).
    pub week_billable: u64,
    /// Estimated remaining percent vs CLAUDE_WEEKLY_BUDGET (0–100).
    pub remaining_percent: f64,
}

/// Codex rate-limit snapshot, read from the most recent `token_count` event.
#[derive(Debug, Clone, Serialize)]
pub struct CodexUsage {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at: i64,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub claude: ClaudeUsage,
    pub codex: Option<CodexUsage>,
}

pub fn build() -> Usage {
    let sources = default_sources();
    Usage {
        claude: claude_usage(&sources.claude_root, Utc::now()),
        codex: codex_usage(&sources.codex_root),
    }
}

/// Per-model pricing in USD per token: (input, output, cache_write, cache_read).
fn pricing(model: &str) -> (f64, f64, f64, f64) {
    let m = model.to_lowercase();
    if m.contains("haiku") {
        (0.80e-6, 4.0e-6, 1.0e-6, 0.08e-6)
    } else if m.contains("sonnet") {
        (3.0e-6, 15.0e-6, 3.75e-6, 0.30e-6)
    } else {
        // opus / default
        (15.0e-6, 75.0e-6, 18.75e-6, 1.50e-6)
    }
}

fn mtime_within(path: &Path, since: DateTime<Utc>) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64 >= since.timestamp())
        .unwrap_or(false)
}

fn claude_files(root: &Path, out: &mut Vec<std::path::PathBuf>, since: DateTime<Utc>) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            claude_files(&p, out, since);
        } else if p.extension().map_or(false, |x| x == "jsonl") && mtime_within(&p, since) {
            out.push(p);
        }
    }
}

fn claude_usage(root: &Path, now: DateTime<Utc>) -> ClaudeUsage {
    let week_ago = now - Duration::days(7);
    let day_ago = now - Duration::days(1);

    let mut files = Vec::new();
    // A file touched within a day past the window may still hold in-window messages.
    claude_files(root, &mut files, now - Duration::days(8));

    let mut u = ClaudeUsage::default();
    for f in files {
        let Ok(content) = fs::read_to_string(&f) else { continue };
        for line in content.lines() {
            if !line.contains("\"usage\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) else { continue };
            let Ok(ts) = DateTime::parse_from_rfc3339(ts) else { continue };
            let ts = ts.with_timezone(&Utc);
            if ts < week_ago {
                continue;
            }
            let msg = v.get("message");
            let usage = msg.and_then(|m| m.get("usage"));
            let Some(usage) = usage else { continue };
            let model = msg
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
                .unwrap_or("opus");

            let inp = usage.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
            let out = usage.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
            let cw = usage
                .get("cache_creation_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let cr = usage
                .get("cache_read_input_tokens")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let tokens = inp + out + cw + cr;
            let billable = inp + out + cw; // cache-reads excluded
            let (pi, po, pcw, pcr) = pricing(model);
            let cost =
                inp as f64 * pi + out as f64 * po + cw as f64 * pcw + cr as f64 * pcr;

            u.week_tokens += tokens;
            u.week_cost += cost;
            u.week_billable += billable;
            if ts >= day_ago {
                u.today_tokens += tokens;
                u.today_cost += cost;
            }
        }
    }
    u.remaining_percent =
        (100.0 * (1.0 - u.week_billable as f64 / CLAUDE_WEEKLY_BUDGET as f64)).clamp(0.0, 100.0);
    u
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
        for line in content.lines() {
            if !line.contains("\"rate_limits\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let rl = v.pointer("/payload/rate_limits");
            let Some(rl) = rl else { continue };
            let primary = rl.get("primary");
            let Some(primary) = primary else { continue };
            latest = Some(CodexUsage {
                used_percent: primary.get("used_percent").and_then(|x| x.as_f64()).unwrap_or(0.0),
                window_minutes: primary
                    .get("window_minutes")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0),
                resets_at: primary.get("resets_at").and_then(|x| x.as_i64()).unwrap_or(0),
                plan_type: rl.get("plan_type").and_then(|x| x.as_str()).map(String::from),
            });
        }
        if latest.is_some() {
            return latest;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_tiers_differ() {
        assert!(pricing("claude-opus-4-8").1 > pricing("claude-sonnet-5").1);
        assert!(pricing("claude-sonnet-5").1 > pricing("claude-haiku-4-5").1);
    }

    #[test]
    fn build_does_not_panic() {
        let _ = build();
    }
}
