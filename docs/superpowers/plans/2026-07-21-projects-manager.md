# projects_manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a macOS always-on-top floating panel that lists every recent Claude Code and Codex project, shows each one's live status, and resumes a session in a terminal with one click.

**Architecture:** A Tauri v2 app. A Rust backend reads the head+tail of Claude Code (`~/.claude/projects/*/*.jsonl`) and Codex (`~/.codex/sessions/**/rollout-*.jsonl`) session files, groups sessions into projects by working directory, enumerates live `claude`/`codex` processes to derive status, and launches a terminal to resume a session. A vanilla web frontend renders a grid of project tiles and calls Rust commands. A 2-second polling loop pushes fresh snapshots to the UI.

**Tech Stack:** Tauri v2, Rust (stable via rustup), `serde`/`serde_json`, `chrono`, `sysinfo` 0.30, `tauri-plugin-window-state` 2; vanilla JS/HTML/CSS frontend (Vite); AppleScript via `osascript` for terminal launch.

## Global Constraints

- Target: macOS (developed on 26.3, Apple Silicon). Node 22 is installed; Rust is installed in Task 0 via rustup.
- Tauri **v2** APIs only (frontend: `@tauri-apps/api/core`, `@tauri-apps/api/event`, `@tauri-apps/api/window`; Rust: `Emitter`/`Manager` traits).
- Crate versions (pin exactly): `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `chrono = { version = "0.4", features = ["serde"] }`, `sysinfo = "0.30"`, `tauri-plugin-window-state = "2"`.
- Session files may be large: **never read a whole file** — read only a bounded head (64 KB) and tail (64 KB).
- Recency window: only sessions with file mtime within the last **30 days** are shown.
- Refresh cadence: backend recomputes and emits a snapshot every **2 seconds** (polling; no filesystem watcher in v1).
- Default terminal: **iTerm2**; fallback **Terminal.app**.
- All backend Rust modules live in `app/src-tauri/src/`. The app is scaffolded under `app/` so the repo root keeps `docs/` clean.

---

### Task 0: Scaffold the Tauri app as a floating panel

**Files:**
- Create (via CLI): `app/` (Tauri v2 vanilla template)
- Modify: `app/src-tauri/tauri.conf.json`
- Modify: `app/src-tauri/Cargo.toml`
- Modify: `app/src-tauri/src/lib.rs`

**Interfaces:**
- Produces: a buildable Tauri app with a borderless, always-on-top, position-remembering window; `pub fn run()` in `lib.rs` as the app entry point.

- [ ] **Step 1: Install Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo --version   # expect: cargo 1.x
```

- [ ] **Step 2: Scaffold the Tauri app (non-interactive)**

```bash
cd /Volumes/GBExDisk/MacOffload/my_project/projects_manager
npm create tauri-app@latest app -- --template vanilla --manager npm --yes
cd app && npm install
```
Expected: `app/` contains `src/` (frontend), `src-tauri/` (Rust), `package.json`.

- [ ] **Step 3: Add backend dependencies**

Edit `app/src-tauri/Cargo.toml`, add under `[dependencies]` (keep the `tauri` line the scaffold generated):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
sysinfo = "0.30"
tauri-plugin-window-state = "2"
```

- [ ] **Step 4: Configure the window as a floating panel**

In `app/src-tauri/tauri.conf.json`, replace the `app.windows[0]` object with:

```json
{
  "title": "Projects",
  "width": 440,
  "height": 640,
  "resizable": true,
  "decorations": false,
  "alwaysOnTop": true,
  "transparent": false,
  "visible": true
}
```

- [ ] **Step 5: Register the window-state plugin**

In `app/src-tauri/src/lib.rs`, inside `run()`, add the plugin to the builder chain (before `.run(...)`):

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_window_state::Builder::default().build())
    // ...existing generate_handler / setup...
```

- [ ] **Step 6: Run the app and verify the floating panel**

```bash
cd /Volumes/GBExDisk/MacOffload/my_project/projects_manager/app
npm run tauri dev
```
Expected: a small borderless window appears, stays above other windows, and its position/size is restored on relaunch. Close it.

- [ ] **Step 7: Commit**

```bash
cd /Volumes/GBExDisk/MacOffload/my_project/projects_manager
git add -A
git commit -m "chore: scaffold Tauri floating panel app"
```

---

### Task 1: Core data model

**Files:**
- Create: `app/src-tauri/src/model.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod model;`)

**Interfaces:**
- Produces: `Agent`, `Status`, `LastEvent`, `Session`, `Project`, and `Status::rank`.

- [ ] **Step 1: Write the failing test**

Append to `app/src-tauri/src/model.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_rank_orders_needs_you_first() {
        assert!(Status::NeedsYou.rank() < Status::Working.rank());
        assert!(Status::Working.rank() < Status::Resumable.rank());
        assert!(Status::Resumable.rank() < Status::Offline.rank());
    }

    #[test]
    fn status_serializes_snake_case() {
        let j = serde_json::to_string(&Status::NeedsYou).unwrap();
        assert_eq!(j, "\"needs_you\"");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test model::`
Expected: FAIL — `Status`, `rank` not found.

- [ ] **Step 3: Write the model**

Prepend to `app/src-tauri/src/model.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NeedsYou,
    Working,
    Resumable,
    Offline,
}

impl Status {
    /// Lower rank = more urgent. Used for sorting tiles and aggregating projects.
    pub fn rank(&self) -> u8 {
        match self {
            Status::NeedsYou => 0,
            Status::Working => 1,
            Status::Resumable => 2,
            Status::Offline => 3,
        }
    }
}

/// Classification of a session's last turn, derived by the parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEvent {
    Working,  // mid-turn (agent still producing)
    Finished, // turn complete (waiting for the user)
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub title: Option<String>,
    pub model: Option<String>,
    pub cwd: String,
    pub branch: Option<String>,
    pub file_path: String,
    pub last_activity: DateTime<Utc>,
    #[serde(skip)]
    pub last_event: LastEvent,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub path: String,
    pub name: String,
    pub branch: Option<String>,
    pub mounted: bool,
    pub status: Status,
    pub last_activity: DateTime<Utc>,
    pub sessions: Vec<Session>,
}
```

Add `mod model;` to `app/src-tauri/src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app/src-tauri && cargo test model::`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/model.rs app/src-tauri/src/lib.rs
git commit -m "feat: add core data model"
```

---

### Task 2: Claude Code session parser

**Files:**
- Create: `app/src-tauri/src/parser_claude.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod parser_claude;`)

**Interfaces:**
- Consumes: `model::{Session, Agent, Status, LastEvent}`.
- Produces: `pub fn parse_claude_content(content: &str, id: &str, file_path: &str) -> Option<Session>` and `pub fn parse_claude_file(path: &std::path::Path) -> Option<Session>`.
- A parsed Claude `Session` has `status = Status::Resumable` as a placeholder (Task 6 overrides it).

- [ ] **Step 1: Write the failing test**

Create `app/src-tauri/src/parser_claude.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test parser_claude::`
Expected: FAIL — `parse_claude_content` not found.

- [ ] **Step 3: Write the parser**

Prepend to `app/src-tauri/src/parser_claude.rs`:

```rust
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
```

Add `mod parser_claude;` to `app/src-tauri/src/lib.rs`. (`io_util::read_head_tail` is created in Task 4; until then this task's tests exercise `parse_claude_content`, which does not call it.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app/src-tauri && cargo test parser_claude::`
Expected: PASS (2 tests). If the compiler errors on the missing `io_util` module, add a temporary `mod io_util { use std::path::Path; pub fn read_head_tail(_p:&Path,_n:usize)->Option<String>{None} }` stub to `lib.rs`; Task 4 replaces it with the real module.

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/parser_claude.rs app/src-tauri/src/lib.rs
git commit -m "feat: add Claude Code session parser"
```

---

### Task 3: Codex session parser

**Files:**
- Create: `app/src-tauri/src/parser_codex.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod parser_codex;`)

**Interfaces:**
- Consumes: `model::{Session, Agent, Status, LastEvent}`.
- Produces: `pub fn parse_codex_content(content: &str, file_path: &str) -> Option<Session>` and `pub fn parse_codex_file(path: &std::path::Path) -> Option<Session>`. Session `id` comes from `session_meta.payload.session_id`; `title` is `None` here (Task 4 fills it from `session_index.jsonl`).

- [ ] **Step 1: Write the failing test**

Create `app/src-tauri/src/parser_codex.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test parser_codex::`
Expected: FAIL — `parse_codex_content` not found.

- [ ] **Step 3: Write the parser**

Prepend to `app/src-tauri/src/parser_codex.rs`:

```rust
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
```

Add `mod parser_codex;` to `app/src-tauri/src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app/src-tauri && cargo test parser_codex::`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/parser_codex.rs app/src-tauri/src/lib.rs
git commit -m "feat: add Codex session parser"
```

---

### Task 4: Bounded file reader + discovery/grouping

**Files:**
- Create: `app/src-tauri/src/io_util.rs`
- Create: `app/src-tauri/src/discovery.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod io_util; mod discovery;`; remove the temporary `io_util` stub from Task 2)

**Interfaces:**
- Consumes: `parser_claude::parse_claude_file`, `parser_codex::parse_codex_file`, `model::{Session, Project, Status}`.
- Produces:
  - `io_util::read_head_tail(path: &Path, cap: usize) -> Option<String>`
  - `discovery::Sources { claude_root, codex_root, codex_index }` and `discovery::default_sources() -> Sources`
  - `discovery::codex_titles(index_path: &Path) -> std::collections::HashMap<String, String>`
  - `discovery::collect_sessions(sources: &Sources, since: DateTime<Utc>) -> Vec<Session>`
  - `discovery::build_projects(sessions: Vec<Session>) -> Vec<Project>`

- [ ] **Step 1: Write the failing test for grouping**

Create `app/src-tauri/src/discovery.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test discovery::`
Expected: FAIL — `build_projects` not found.

- [ ] **Step 3: Write `io_util`**

Create `app/src-tauri/src/io_util.rs`:

```rust
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Read up to `cap` bytes from the start and `cap` bytes from the end of a file,
/// joined with a newline. Small files are returned whole. Lossy UTF-8.
pub fn read_head_tail(path: &Path, cap: usize) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len() as usize;

    if len <= cap * 2 {
        let mut buf = Vec::with_capacity(len);
        f.read_to_end(&mut buf).ok()?;
        return Some(String::from_utf8_lossy(&buf).into_owned());
    }

    let mut head = vec![0u8; cap];
    f.read_exact(&mut head).ok()?;

    let mut tail = vec![0u8; cap];
    f.seek(SeekFrom::End(-(cap as i64))).ok()?;
    f.read_exact(&mut tail).ok()?;

    let mut out = String::from_utf8_lossy(&head).into_owned();
    out.push('\n');
    out.push_str(&String::from_utf8_lossy(&tail));
    Some(out)
}
```

- [ ] **Step 4: Write `discovery`**

Prepend to `app/src-tauri/src/discovery.rs`:

```rust
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
```

Add `mod io_util; mod discovery;` to `lib.rs` and delete the temporary `io_util` stub added in Task 2.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd app/src-tauri && cargo test discovery:: parser_claude:: parser_codex::`
Expected: PASS (all).

- [ ] **Step 6: Commit**

```bash
git add app/src-tauri/src/io_util.rs app/src-tauri/src/discovery.rs app/src-tauri/src/lib.rs
git commit -m "feat: add bounded file reader and session discovery/grouping"
```

---

### Task 5: Live-process scanner

**Files:**
- Create: `app/src-tauri/src/procs.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod procs;`)

**Interfaces:**
- Produces: `procs::live_agent_cwds() -> std::collections::HashSet<std::path::PathBuf>` — the set of working directories that currently have a live `claude` or `codex` process.

- [ ] **Step 1: Write the test (integration-style, non-strict)**

Create `app/src-tauri/src/procs.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_set_without_panicking() {
        // Cannot assert specific contents in CI, but the call must succeed
        // and every entry must be an absolute path.
        let cwds = live_agent_cwds();
        for p in &cwds {
            assert!(p.is_absolute(), "cwd should be absolute: {:?}", p);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test procs::`
Expected: FAIL — `live_agent_cwds` not found.

- [ ] **Step 3: Write the scanner**

Prepend to `app/src-tauri/src/procs.rs`:

```rust
use std::collections::HashSet;
use std::path::PathBuf;
use sysinfo::System;

/// Working directories of every live `claude`/`codex` process.
pub fn live_agent_cwds() -> HashSet<PathBuf> {
    let mut sys = System::new();
    sys.refresh_processes();

    let mut set = HashSet::new();
    for process in sys.processes().values() {
        let name = process.name();
        if name == "claude" || name == "codex" {
            let cwd = process.cwd();
            if !cwd.as_os_str().is_empty() {
                set.insert(cwd.to_path_buf());
            }
        }
    }
    set
}
```

Add `mod procs;` to `lib.rs`.

Note on `sysinfo` 0.30 API: `System::new()`, `sys.refresh_processes()`, `process.name() -> &str`, `process.cwd() -> &Path`. If a pinned build resolves a different minor with a changed signature, adjust only inside this function; the test above is the contract.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app/src-tauri && cargo test procs::`
Expected: PASS. (With a `claude` running, `live_agent_cwds()` will contain its cwd.)

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/procs.rs app/src-tauri/src/lib.rs
git commit -m "feat: add live agent process scanner"
```

---

### Task 6: Status combiner

**Files:**
- Create: `app/src-tauri/src/status.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod status;`)

**Interfaces:**
- Consumes: `model::{Session, Status, LastEvent}`.
- Produces: `status::apply(session: &mut Session, live: &std::collections::HashSet<std::path::PathBuf>)` — sets `session.status` from mount state, live-process membership, and `last_event`.

- [ ] **Step 1: Write the failing test**

Create `app/src-tauri/src/status.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, LastEvent, Session, Status};
    use chrono::Utc;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn make(cwd: &str, ev: LastEvent) -> Session {
        Session {
            id: "s".into(),
            agent: Agent::Claude,
            title: None,
            model: None,
            cwd: cwd.to_string(),
            branch: None,
            file_path: "/tmp".into(),
            last_activity: Utc::now(),
            last_event: ev,
            status: Status::Resumable,
        }
    }

    #[test]
    fn unmounted_path_is_offline() {
        let mut s = make("/no/such/path/xyz123", LastEvent::Finished);
        apply(&mut s, &HashSet::new());
        assert_eq!(s.status, Status::Offline);
    }

    #[test]
    fn mounted_no_process_is_resumable() {
        let mut s = make("/tmp", LastEvent::Finished);
        apply(&mut s, &HashSet::new());
        assert_eq!(s.status, Status::Resumable);
    }

    #[test]
    fn live_and_finished_is_needs_you() {
        let mut s = make("/tmp", LastEvent::Finished);
        let mut live = HashSet::new();
        live.insert(PathBuf::from("/tmp"));
        apply(&mut s, &live);
        assert_eq!(s.status, Status::NeedsYou);
    }

    #[test]
    fn live_and_working_is_working() {
        let mut s = make("/tmp", LastEvent::Working);
        let mut live = HashSet::new();
        live.insert(PathBuf::from("/tmp"));
        apply(&mut s, &live);
        assert_eq!(s.status, Status::Working);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test status::`
Expected: FAIL — `apply` not found.

- [ ] **Step 3: Write the combiner**

Prepend to `app/src-tauri/src/status.rs`:

```rust
use crate::model::{LastEvent, Session, Status};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn apply(session: &mut Session, live: &HashSet<PathBuf>) {
    let cwd = Path::new(&session.cwd);
    if !cwd.exists() {
        session.status = Status::Offline;
        return;
    }
    let is_live = live.contains(cwd);
    session.status = if !is_live {
        Status::Resumable
    } else {
        match session.last_event {
            LastEvent::Finished => Status::NeedsYou,
            LastEvent::Working | LastEvent::Unknown => Status::Working,
        }
    };
}
```

Add `mod status;` to `lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd app/src-tauri && cargo test status::`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add app/src-tauri/src/status.rs app/src-tauri/src/lib.rs
git commit -m "feat: add status combiner"
```

---

### Task 7: Snapshot assembly + `get_snapshot` command

**Files:**
- Create: `app/src-tauri/src/snapshot.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod snapshot;`, register command)

**Interfaces:**
- Consumes: `discovery`, `procs`, `status`, `model::Project`.
- Produces: `snapshot::build() -> Vec<Project>` (sessions status-applied, projects sorted NeedsYou-first then most-recent) and a Tauri command `get_snapshot() -> Vec<Project>`.

- [ ] **Step 1: Write the failing test**

Create `app/src-tauri/src/snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_runs_and_returns_sorted_projects() {
        // Uses the real home dir; must not panic and must be sorted by urgency then recency.
        let projects = build();
        for w in projects.windows(2) {
            let a = &w[0];
            let b = &w[1];
            let a_key = (a.status.rank(), std::cmp::Reverse(a.last_activity));
            let b_key = (b.status.rank(), std::cmp::Reverse(b.last_activity));
            assert!(a_key <= b_key, "projects must be sorted by urgency then recency");
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test snapshot::`
Expected: FAIL — `build` not found.

- [ ] **Step 3: Write snapshot assembly**

Prepend to `app/src-tauri/src/snapshot.rs`:

```rust
use crate::discovery::{build_projects, collect_sessions, default_sources};
use crate::model::Project;
use crate::procs::live_agent_cwds;
use crate::status;
use chrono::{Duration, Utc};

pub fn build() -> Vec<Project> {
    let sources = default_sources();
    let since = Utc::now() - Duration::days(30);
    let mut sessions = collect_sessions(&sources, since);

    let live = live_agent_cwds();
    for s in &mut sessions {
        status::apply(s, &live);
    }

    // Recompute each project's aggregate status now that sessions are classified.
    let mut projects = build_projects(sessions);
    for p in &mut projects {
        if let Some(min) = p.sessions.iter().min_by_key(|s| s.status.rank()) {
            p.status = min.status;
        }
    }

    projects.sort_by_key(|p| (p.status.rank(), std::cmp::Reverse(p.last_activity)));
    projects
}
```

- [ ] **Step 4: Add the Tauri command**

In `app/src-tauri/src/lib.rs`, add:

```rust
#[tauri::command]
fn get_snapshot() -> Vec<model::Project> {
    snapshot::build()
}
```

And register it in the builder: `.invoke_handler(tauri::generate_handler![get_snapshot])` (merge with any existing handler list). Add `mod snapshot;`.

- [ ] **Step 5: Run test + build**

Run: `cd app/src-tauri && cargo test snapshot:: && cargo build`
Expected: PASS and a clean build.

- [ ] **Step 6: Commit**

```bash
git add app/src-tauri/src/snapshot.rs app/src-tauri/src/lib.rs
git commit -m "feat: assemble snapshot and expose get_snapshot command"
```

---

### Task 8: Terminal launcher

**Files:**
- Create: `app/src-tauri/src/launcher.rs`
- Modify: `app/src-tauri/src/lib.rs` (add `mod launcher;`, register commands)

**Interfaces:**
- Consumes: `model::Agent`.
- Produces:
  - `launcher::shell_quote(s: &str) -> String`
  - `launcher::build_resume_command(agent: Agent, session_id: &str, fresh: bool) -> String`
  - `launcher::build_shell_line(cwd: &str, agent: Agent, session_id: &str, fresh: bool) -> String`
  - `launcher::launch(shell_line: &str, terminal: &str) -> Result<(), String>` (`terminal` is `"iterm"` or `"terminal"`)
  - Tauri commands `resume_session(...)` and `open_folder(path)`.

- [ ] **Step 1: Write the failing test**

Create `app/src-tauri/src/launcher.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Agent;

    #[test]
    fn resume_commands_per_agent() {
        assert_eq!(build_resume_command(Agent::Claude, "abc", false), "claude --resume abc");
        assert_eq!(build_resume_command(Agent::Claude, "abc", true), "claude");
        assert_eq!(build_resume_command(Agent::Codex, "xyz", false), "codex resume xyz");
        assert_eq!(build_resume_command(Agent::Codex, "xyz", true), "codex");
    }

    #[test]
    fn shell_line_quotes_cwd_with_spaces() {
        let line = build_shell_line("/Volumes/My Disk/proj", Agent::Claude, "id1", false);
        assert_eq!(line, "cd '/Volumes/My Disk/proj' && claude --resume id1");
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd app/src-tauri && cargo test launcher::`
Expected: FAIL — functions not found.

- [ ] **Step 3: Write the launcher**

Prepend to `app/src-tauri/src/launcher.rs`:

```rust
use crate::model::Agent;
use std::process::Command;

/// POSIX single-quote a string for safe inclusion in a shell command.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub fn build_resume_command(agent: Agent, session_id: &str, fresh: bool) -> String {
    match (agent, fresh) {
        (Agent::Claude, true) => "claude".to_string(),
        (Agent::Claude, false) => format!("claude --resume {session_id}"),
        (Agent::Codex, true) => "codex".to_string(),
        (Agent::Codex, false) => format!("codex resume {session_id}"),
    }
}

pub fn build_shell_line(cwd: &str, agent: Agent, session_id: &str, fresh: bool) -> String {
    format!(
        "cd {} && {}",
        shell_quote(cwd),
        build_resume_command(agent, session_id, fresh)
    )
}

/// Open the given shell line in a new terminal window via AppleScript.
pub fn launch(shell_line: &str, terminal: &str) -> Result<(), String> {
    let script = match terminal {
        "terminal" => format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            applescript_escape(shell_line)
        ),
        _ => format!(
            "tell application \"iTerm\"\nactivate\nset w to (create window with default profile)\ntell current session of w to write text \"{}\"\nend tell",
            applescript_escape(shell_line)
        ),
    };
    let status = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .status()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("osascript exited with status {status}"))
    }
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
```

- [ ] **Step 4: Add Tauri commands**

In `app/src-tauri/src/lib.rs`:

```rust
#[tauri::command]
fn resume_session(
    cwd: String,
    agent: model::Agent,
    session_id: String,
    fresh: bool,
    terminal: String,
) -> Result<(), String> {
    let line = launcher::build_shell_line(&cwd, agent, &session_id, fresh);
    launcher::launch(&line, &terminal)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&path)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}
```

Register both in `generate_handler![get_snapshot, resume_session, open_folder]`. Add `mod launcher;`.

- [ ] **Step 5: Run tests + build**

Run: `cd app/src-tauri && cargo test launcher:: && cargo build`
Expected: PASS and clean build.

- [ ] **Step 6: Manual smoke test**

Add a temporary throwaway call or run the app (after Task 9/10) to confirm a real iTerm2 window opens at a project path. For now verify the command string only.

- [ ] **Step 7: Commit**

```bash
git add app/src-tauri/src/launcher.rs app/src-tauri/src/lib.rs
git commit -m "feat: add terminal launcher and resume/open commands"
```

---

### Task 9: 2-second polling loop that emits snapshots

**Files:**
- Modify: `app/src-tauri/src/lib.rs` (spawn poller in `setup`)

**Interfaces:**
- Consumes: `snapshot::build`, Tauri `Emitter`.
- Produces: a background thread that emits a `"snapshot"` event (payload `Vec<Project>`) every 2 seconds.

- [ ] **Step 1: Add the poller in `setup`**

In `app/src-tauri/src/lib.rs`, use the app handle in `.setup(...)`:

```rust
use tauri::{Emitter, Manager};

// inside Builder chain:
.setup(|app| {
    let handle = app.handle().clone();
    std::thread::spawn(move || loop {
        let projects = snapshot::build();
        let _ = handle.emit("snapshot", projects);
        std::thread::sleep(std::time::Duration::from_secs(2));
    });
    Ok(())
})
```

- [ ] **Step 2: Build**

Run: `cd app/src-tauri && cargo build`
Expected: clean build.

- [ ] **Step 3: Verify events fire**

Run `npm run tauri dev` from `app/`, open the webview devtools console, and temporarily add in the frontend `main.js`:
```js
import { listen } from '@tauri-apps/api/event';
listen('snapshot', e => console.log('snapshot', e.payload.length));
```
Expected: a log line every ~2s with a project count. Remove the temporary log after confirming.

- [ ] **Step 4: Commit**

```bash
git add app/src-tauri/src/lib.rs app/src/main.js
git commit -m "feat: emit project snapshot every 2 seconds"
```

---

### Task 10: Frontend — project tile grid

**Files:**
- Modify: `app/index.html`
- Modify: `app/src/main.js`
- Modify: `app/src/styles.css`

**Interfaces:**
- Consumes: `get_snapshot` command, `"snapshot"` event, `resume_session`/`open_folder` commands.
- Produces: a rendered, sorted, searchable grid of project tiles with status colors.

- [ ] **Step 1: Replace `index.html` body**

Set the `<body>` of `app/index.html` to:

```html
<body>
  <header id="bar">
    <input id="search" type="search" placeholder="Filter projects…" />
    <button id="pin" title="Toggle always on top">📌</button>
  </header>
  <main id="grid"></main>
  <script type="module" src="/src/main.js"></script>
</body>
```

- [ ] **Step 2: Write `main.js`**

Replace `app/src/main.js` with:

```js
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

const grid = document.getElementById('grid');
const search = document.getElementById('search');
const pin = document.getElementById('pin');

let projects = [];
let filter = '';
let pinned = true;

const STATUS_LABEL = {
  needs_you: 'Needs you',
  working: 'Working',
  resumable: 'Resumable',
  offline: 'Offline',
};

function relTime(iso) {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

function agentIcons(sessions) {
  const set = new Set(sessions.map((s) => s.agent));
  return [...set].map((a) => (a === 'claude' ? 'C' : 'X')).join(' ');
}

function render() {
  const q = filter.trim().toLowerCase();
  grid.innerHTML = '';
  for (const p of projects) {
    const hay = `${p.name} ${(p.sessions[0]?.title) || ''}`.toLowerCase();
    if (q && !hay.includes(q)) continue;

    const top = p.sessions[0] || {};
    const tile = document.createElement('button');
    tile.className = `tile ${p.status}`;
    tile.disabled = p.status === 'offline';
    tile.innerHTML = `
      <div class="tile-head">
        <span class="name">${p.name}</span>
        <span class="agents">${agentIcons(p.sessions)}</span>
      </div>
      <div class="status-row">
        <span class="dot"></span>
        <span class="status">${STATUS_LABEL[p.status]}</span>
        <span class="time">${relTime(p.last_activity)}</span>
      </div>
      <div class="title">${(top.title || '').replace(/</g, '&lt;')}</div>
      <div class="actions">
        <span class="resume">Resume ▸</span>
        <span class="folder" title="Open folder">📁</span>
      </div>`;

    tile.querySelector('.resume').onclick = (e) => {
      e.stopPropagation();
      if (top.id) {
        invoke('resume_session', {
          cwd: p.path,
          agent: top.agent,
          sessionId: top.id,
          fresh: false,
          terminal: 'iterm',
        });
      }
    };
    tile.querySelector('.folder').onclick = (e) => {
      e.stopPropagation();
      invoke('open_folder', { path: p.path });
    };
    tile.onclick = () => {
      if (top.id) {
        invoke('resume_session', {
          cwd: p.path,
          agent: top.agent,
          sessionId: top.id,
          fresh: false,
          terminal: 'iterm',
        });
      }
    };
    grid.appendChild(tile);
  }
}

search.addEventListener('input', () => {
  filter = search.value;
  render();
});

pin.addEventListener('click', async () => {
  pinned = !pinned;
  await getCurrentWindow().setAlwaysOnTop(pinned);
  pin.style.opacity = pinned ? '1' : '0.4';
});

listen('snapshot', (e) => {
  projects = e.payload;
  render();
});

invoke('get_snapshot').then((p) => {
  projects = p;
  render();
});
```

- [ ] **Step 3: Write `styles.css`**

Replace `app/src/styles.css` with:

```css
:root {
  --bg: #16171c;
  --tile: #21232b;
  --text: #e7e9ee;
  --muted: #8a8f9c;
  --needs: #ffb020;
  --working: #35c759;
  --resumable: #5b8def;
  --offline: #4a4d57;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  font: 13px -apple-system, system-ui, sans-serif;
  background: var(--bg);
  color: var(--text);
  user-select: none;
}
#bar {
  display: flex;
  gap: 8px;
  padding: 8px;
  position: sticky;
  top: 0;
  background: var(--bg);
}
#search {
  flex: 1;
  background: var(--tile);
  border: none;
  border-radius: 8px;
  padding: 6px 10px;
  color: var(--text);
}
#pin { background: none; border: none; cursor: pointer; font-size: 15px; }
#grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 8px;
  padding: 8px;
}
.tile {
  text-align: left;
  background: var(--tile);
  border: 1px solid transparent;
  border-radius: 12px;
  padding: 10px;
  color: var(--text);
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tile:hover { border-color: #3a3d47; }
.tile.needs_you { box-shadow: 0 0 0 1px var(--needs), 0 0 12px rgba(255,176,32,0.25); }
.tile-head { display: flex; justify-content: space-between; align-items: center; }
.name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.agents { color: var(--muted); font-size: 11px; }
.status-row { display: flex; align-items: center; gap: 6px; color: var(--muted); }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--resumable); }
.needs_you .dot { background: var(--needs); }
.working .dot { background: var(--working); }
.resumable .dot { background: var(--resumable); }
.offline .dot { background: var(--offline); }
.time { margin-left: auto; }
.title {
  color: var(--muted);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.actions { display: flex; justify-content: space-between; margin-top: 2px; }
.resume { color: var(--resumable); }
.folder { cursor: pointer; }
.tile.offline { opacity: 0.5; cursor: default; }
```

- [ ] **Step 4: Run the app end-to-end**

Run: `cd app && npm run tauri dev`
Expected: the floating panel shows real project tiles from your machine, sorted with any "Needs you" projects (highlighted) first, then most recent. Typing in the filter narrows the list. Clicking a tile opens iTerm2 at that project running the resume command. The 📁 opens the folder. The 📌 toggles always-on-top.

- [ ] **Step 5: Commit**

```bash
git add app/index.html app/src/main.js app/src/styles.css
git commit -m "feat: project tile grid frontend"
```

---

### Task 11: Package a runnable app + README

**Files:**
- Create: `README.md`
- Modify: `app/src-tauri/tauri.conf.json` (product name/identifier if still default)

**Interfaces:**
- Produces: a `.app` bundle and usage docs.

- [ ] **Step 1: Set product identity**

In `app/src-tauri/tauri.conf.json`, set `"productName": "Projects Manager"` and a real `"identifier"` such as `"com.duclv.projectsmanager"`.

- [ ] **Step 2: Build the release bundle**

Run: `cd app && npm run tauri build`
Expected: a `.app` (and `.dmg`) under `app/src-tauri/target/release/bundle/`. Launch the `.app` and confirm the panel works.

- [ ] **Step 3: Write `README.md`**

Create `README.md` at repo root:

```markdown
# Projects Manager

Always-on-top macOS panel that lists your recent Claude Code and Codex projects,
shows each one's live status, and resumes a session in iTerm2 with one click.

## Status meanings
- 🟠 Needs you — an agent finished its turn and is waiting for input
- 🟢 Working — an agent is running right now
- ⚪ Resumable — click to resume in a terminal
- 🚫 Offline — the project's folder is on an unmounted volume

## Develop
```
cd app
npm install
npm run tauri dev
```

## Build
```
cd app
npm run tauri build
```
The bundled app is in `app/src-tauri/target/release/bundle/`.

## Data sources (read-only)
- `~/.claude/projects/*/*.jsonl`
- `~/.codex/sessions/**/rollout-*.jsonl` and `~/.codex/session_index.jsonl`
```

- [ ] **Step 4: Commit**

```bash
git add README.md app/src-tauri/tauri.conf.json
git commit -m "chore: package app and add README"
```

---

## Self-Review Notes

- **Spec coverage:** discovery (Task 4), parsers with status classification (Tasks 2–3), process→cwd via `sysinfo` (Task 5), status model (Task 6), snapshot + command (Task 7), launcher/iTerm2 (Task 8), live refresh (Task 9 — polling instead of `notify`, an intentional v1 simplification noted in Global Constraints), tile grid with NeedsYou-first sort + highlight + search (Task 10), Offline handling (Tasks 4/6/10), window position persistence + always-on-top (Task 0/10). Testing strategy from the spec is realized as inline `#[cfg(test)]` modules for parser, status, discovery, and launcher command-strings.
- **Deviations from spec (intentional):** (1) `sysinfo` for process cwd instead of `libproc`/`lsof` — same native mechanism, works on external volumes, one crate. (2) 2-second polling instead of `notify` filesystem watcher — simpler, equally responsive for this use; `notify` remains a v2 option. Both are recorded in Global Constraints.
- **Deferred (per spec v2):** native desktop notifications; focusing an already-open terminal; per-launch terminal choice.
