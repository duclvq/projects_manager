use crate::io_util::read_head_tail;
use crate::model::{Agent, LastEvent, Session, Status};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;

pub fn parse_claude_file(path: &Path) -> Option<Session> {
    let content = read_head_tail(path, 64 * 1024)?;
    let id = path.file_stem()?.to_string_lossy().to_string();
    parse_claude_content(&content, &id, &path.to_string_lossy())
}

pub fn parse_claude_content(content: &str, id: &str, file_path: &str) -> Option<Session> {
    let mut cwd: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut model: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut first_user_prompt: Option<String> = None;
    let mut last_ts: Option<DateTime<Utc>> = None;
    let mut last_event = LastEvent::Unknown;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // partial/truncated line from head/tail slicing
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                cwd = Some(c.to_string());
            }
        }
        if branch.is_none() {
            if let Some(b) = v.get("gitBranch").and_then(|b| b.as_str()) {
                if !b.is_empty() {
                    branch = Some(b.to_string());
                }
            }
        }
        if let Some(t) = v.get("timestamp").and_then(|t| t.as_str()) {
            if let Ok(ts) = DateTime::parse_from_rfc3339(t) {
                let ts = ts.with_timezone(&Utc);
                if last_ts.map_or(true, |cur| ts > cur) {
                    last_ts = Some(ts);
                }
            }
        }

        match ty {
            "ai-title" => {
                if let Some(t) = v.get("aiTitle").and_then(|t| t.as_str()) {
                    ai_title = Some(t.to_string());
                }
            }
            "user" => {
                last_event = LastEvent::Working; // a user/tool_result line means the agent will respond
                if first_user_prompt.is_none() {
                    if let Some(c) = v.pointer("/message/content").and_then(|c| c.as_str()) {
                        first_user_prompt = Some(c.to_string());
                    }
                }
            }
            "assistant" => {
                if let Some(m) = v.pointer("/message/model").and_then(|m| m.as_str()) {
                    model = Some(m.to_string());
                }
                let stop = v.pointer("/message/stop_reason").and_then(|s| s.as_str());
                last_event = match stop {
                    Some("end_turn") | Some("stop_sequence") => LastEvent::Finished,
                    _ => LastEvent::Working, // tool_use / max_tokens / null → mid-turn
                };
            }
            _ => {}
        }
    }

    let cwd = cwd?;
    let title = ai_title.or_else(|| first_user_prompt.map(|p| truncate(&p, 80)));
    Some(Session {
        id: id.to_string(),
        agent: Agent::Claude,
        title,
        model,
        cwd,
        branch,
        file_path: file_path.to_string(),
        last_activity: last_ts.unwrap_or_else(Utc::now),
        last_event,
        status: Status::Resumable,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, LastEvent};

    const FIXTURE: &str = r#"{"type":"system","cwd":"/Users/duclv/proj","gitBranch":"main","timestamp":"2026-07-13T12:00:00.000Z","sessionId":"abc"}
{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-07-13T12:00:01.000Z"}
{"type":"assistant","message":{"role":"assistant","model":"claude-opus-4-8","stop_reason":"end_turn","content":[{"type":"text","text":"hi"}]},"timestamp":"2026-07-13T12:16:11.603Z"}
{"type":"ai-title","aiTitle":"Say hello","sessionId":"abc"}"#;

    #[test]
    fn parses_title_cwd_branch_model_and_finished_state() {
        let s = parse_claude_content(FIXTURE, "abc", "/tmp/abc.jsonl").unwrap();
        assert_eq!(s.agent, Agent::Claude);
        assert_eq!(s.id, "abc");
        assert_eq!(s.cwd, "/Users/duclv/proj");
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.title.as_deref(), Some("Say hello"));
        assert_eq!(s.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(s.last_event, LastEvent::Finished);
    }

    #[test]
    fn detects_working_when_last_turn_is_tool_use() {
        let working = r#"{"type":"system","cwd":"/x","timestamp":"2026-07-13T12:00:00.000Z"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash"}]},"timestamp":"2026-07-13T12:00:05.000Z"}"#;
        let s = parse_claude_content(working, "w", "/tmp/w.jsonl").unwrap();
        assert_eq!(s.last_event, LastEvent::Working);
    }
}
