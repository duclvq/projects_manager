use crate::model::{Project, Session};
use crate::parser_claude::parse_claude_file;
use crate::parser_codex::parse_codex_file;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub struct Sources {
    pub claude_root: PathBuf,
    pub codex_root: PathBuf,
    pub codex_index: PathBuf,
}

pub fn default_sources() -> Sources {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let home = PathBuf::from(home);
    Sources {
        claude_root: home.join(".claude/projects"),
        codex_root: home.join(".codex/sessions"),
        codex_index: home.join(".codex/session_index.jsonl"),
    }
}

/// Map Codex session_id -> thread_name from session_index.jsonl.
pub fn codex_titles(index_path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(content) = fs::read_to_string(index_path) {
        for line in content.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(name)) = (
                    v.get("id").and_then(|x| x.as_str()),
                    v.get("thread_name").and_then(|x| x.as_str()),
                ) {
                    map.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    map
}

fn recent_enough(path: &Path, since: DateTime<Utc>) -> bool {
    let modified = match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let secs = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    secs >= since.timestamp()
}

/// Recursively collect `*.jsonl` files whose mtime is >= `since`.
fn recent_jsonl(dir: &Path, since: DateTime<Utc>, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            recent_jsonl(&path, since, out);
        } else if path.extension().map_or(false, |e| e == "jsonl") && recent_enough(&path, since) {
            out.push(path);
        }
    }
}

pub fn collect_sessions(sources: &Sources, since: DateTime<Utc>) -> Vec<Session> {
    let mut sessions = Vec::new();

    let mut claude_files = Vec::new();
    recent_jsonl(&sources.claude_root, since, &mut claude_files);
    for f in claude_files {
        if let Some(s) = parse_claude_file(&f) {
            sessions.push(s);
        }
    }

    let titles = codex_titles(&sources.codex_index);
    let mut codex_files = Vec::new();
    recent_jsonl(&sources.codex_root, since, &mut codex_files);
    for f in codex_files {
        if let Some(mut s) = parse_codex_file(&f) {
            if s.title.is_none() {
                s.title = titles.get(&s.id).cloned();
            }
            sessions.push(s);
        }
    }

    sessions
}

pub fn build_projects(sessions: Vec<Session>) -> Vec<Project> {
    let mut groups: HashMap<String, Vec<Session>> = HashMap::new();
    for s in sessions {
        groups.entry(s.cwd.clone()).or_default().push(s);
    }

    let mut projects = Vec::new();
    for (path, mut group) in groups {
        group.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        let name = Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let branch = group.iter().find_map(|s| s.branch.clone());
        let mounted = Path::new(&path).exists();
        let last_activity = group
            .iter()
            .map(|s| s.last_activity)
            .max()
            .unwrap_or_else(Utc::now);
        let status = group
            .iter()
            .min_by_key(|s| s.status.rank())
            .map(|s| s.status)
            .unwrap_or(crate::model::Status::Resumable);

        projects.push(Project {
            path,
            name,
            branch,
            mounted,
            status,
            last_activity,
            sessions: group,
        });
    }

    projects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, LastEvent, Session, Status};
    use chrono::{TimeZone, Utc};

    fn sess(cwd: &str, agent: Agent, secs: i64, ev: LastEvent) -> Session {
        Session {
            id: format!("{cwd}-{secs}"),
            agent,
            title: None,
            model: None,
            cwd: cwd.to_string(),
            branch: None,
            file_path: "/tmp/x".into(),
            last_activity: Utc.timestamp_opt(secs, 0).unwrap(),
            last_event: ev,
            status: Status::Resumable,
        }
    }

    #[test]
    fn groups_by_cwd_and_sorts_sessions_desc() {
        let sessions = vec![
            sess("/a", Agent::Claude, 100, LastEvent::Finished),
            sess("/a", Agent::Codex, 200, LastEvent::Finished),
            sess("/b", Agent::Claude, 150, LastEvent::Finished),
        ];
        let mut projects = build_projects(sessions);
        projects.sort_by(|x, y| x.path.cmp(&y.path));
        assert_eq!(projects.len(), 2);
        let a = &projects[0];
        assert_eq!(a.path, "/a");
        assert_eq!(a.name, "a");
        assert_eq!(a.sessions.len(), 2);
        assert_eq!(a.sessions[0].last_activity.timestamp(), 200); // newest first
        assert_eq!(a.last_activity.timestamp(), 200);
    }

    #[test]
    fn project_status_is_most_urgent_session() {
        let sessions = vec![
            sess("/a", Agent::Claude, 100, LastEvent::Finished),
            sess("/a", Agent::Codex, 90, LastEvent::Finished),
        ];
        let mut s0 = sessions;
        s0[0].status = Status::Resumable;
        s0[1].status = Status::NeedsYou;
        let projects = build_projects(s0);
        assert_eq!(projects[0].status, Status::NeedsYou);
    }
}
