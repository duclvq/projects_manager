use crate::io_util::read_head_tail;
use crate::model::{Agent, LastEvent, Session, Status};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::Path;

pub fn parse_codex_file(path: &Path) -> Option<Session> {
    let content = read_head_tail(path, 64 * 1024)?;
    parse_codex_content(&content, &path.to_string_lossy())
}

pub fn parse_codex_content(content: &str, file_path: &str) -> Option<Session> {
    let mut id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut last_ts: Option<DateTime<Utc>> = None;
    let mut last_event = LastEvent::Unknown;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let payload = v.get("payload");

        if let Some(t) = v.get("timestamp").and_then(|t| t.as_str()) {
            if let Ok(ts) = DateTime::parse_from_rfc3339(t) {
                let ts = ts.with_timezone(&Utc);
                if last_ts.map_or(true, |cur| ts > cur) {
                    last_ts = Some(ts);
                }
            }
        }

        if ty == "session_meta" {
            if let Some(p) = payload {
                if id.is_none() {
                    id = p.get("session_id").and_then(|s| s.as_str()).map(String::from);
                }
                if cwd.is_none() {
                    cwd = p.get("cwd").and_then(|s| s.as_str()).map(String::from);
                }
                if model.is_none() {
                    model = p.get("model_provider").and_then(|s| s.as_str()).map(String::from);
                }
            }
        } else if ty == "event_msg" {
            match payload.and_then(|p| p.get("type")).and_then(|t| t.as_str()) {
                Some("task_started") => last_event = LastEvent::Working,
                Some("task_complete") => last_event = LastEvent::Finished,
                _ => {}
            }
        }
    }

    Some(Session {
        id: id?,
        agent: Agent::Codex,
        title: None,
        model,
        cwd: cwd?,
        branch: None,
        file_path: file_path.to_string(),
        last_activity: last_ts.unwrap_or_else(Utc::now),
        last_event,
        status: Status::Resumable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, LastEvent};

    const FINISHED: &str = r#"{"timestamp":"2026-07-17T04:48:36.553Z","type":"session_meta","payload":{"session_id":"019f","cwd":"/Volumes/ExFAT","model_provider":"openai"}}
{"timestamp":"2026-07-17T04:49:00.000Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2026-07-17T06:31:22.407Z","type":"event_msg","payload":{"type":"task_complete"}}"#;

    #[test]
    fn parses_meta_and_finished_state() {
        let s = parse_codex_content(FINISHED, "/tmp/r.jsonl").unwrap();
        assert_eq!(s.agent, Agent::Codex);
        assert_eq!(s.id, "019f");
        assert_eq!(s.cwd, "/Volumes/ExFAT");
        assert_eq!(s.model.as_deref(), Some("openai"));
        assert_eq!(s.last_event, LastEvent::Finished);
    }

    #[test]
    fn detects_working_when_task_started_is_last() {
        let working = r#"{"timestamp":"2026-07-17T04:48:36.553Z","type":"session_meta","payload":{"session_id":"z","cwd":"/x"}}
{"timestamp":"2026-07-17T04:49:00.000Z","type":"event_msg","payload":{"type":"task_started"}}"#;
        let s = parse_codex_content(working, "/tmp/z.jsonl").unwrap();
        assert_eq!(s.last_event, LastEvent::Working);
    }
}
