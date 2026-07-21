# projects_manager — floating control panel for coding agents

**Date:** 2026-07-21
**Status:** Approved design, ready for implementation planning

## Problem

The user runs many projects concurrently using two terminal-based coding
agents — Claude Code and Codex. Work is scattered across terminal windows on
internal and external volumes. There is no single place to see which projects
have agents running, which ones are waiting for input, or to quickly jump back
into a session. The user wants a small macOS surface that "lives on the screen"
to monitor and, above all, **quickly resume** work.

## Goals (ranked)

1. **Quick resume / launch (v1 primary).** One click on a project resumes its
   most recent agent session in a terminal at the correct working directory.
2. **Status at a glance.** Each project shows whether an agent is working,
   waiting for the user, or idle/resumable.
3. **Portfolio overview.** All recent projects visible in one grid.

## Non-goals (v1 / YAGNI)

- Sending commands to or controlling running agents (read + launch only).
- Remote/cloud sync, auth, multi-machine.
- Physical macro-pad / hardware integration.
- Native desktop notifications (deferred to v2; v1 uses in-panel highlight).

## Form factor & stack

- **Form:** a single small **always-on-top floating panel** (control-surface /
  Stream-Deck aesthetic). Remembers screen position; toggle for always-on-top.
- **Stack:** **Tauri** — Rust backend, web (HTML/CSS/JS) frontend. Chosen for a
  native, lightweight always-on-top window with a hackable web UI. Node is
  already installed; Rust will be added via rustup during setup.
- **Default terminal:** iTerm2 (Terminal.app selectable; per-launch prompt is a
  future option).

## Data sources (verified on the user's machine)

### Claude Code
- Sessions: `~/.claude/projects/<path-slug>/<session-uuid>.jsonl`, one folder
  per project working directory, one JSONL per session.
- Each record is a JSON object. Relevant record types observed:
  `system` (carries `cwd`, `gitBranch`, `version`), `user`, `assistant`
  (carries `message.role`, `message.stop_reason`, `content` blocks),
  `ai-title` (auto-generated session title), `last-prompt` (`lastPrompt`),
  plus `mode`, `permission-mode`, `attachment`, `file-history-snapshot`,
  `queue-operation`.
- Timestamps are ISO-8601 in `timestamp`.

### Codex
- Sessions: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
- First record `type: session_meta` with `payload.session_id`, `payload.cwd`,
  `payload.cli_version`, `payload.model_provider`, `payload.originator`.
- Subsequent records: `event_msg` and `response_item`, each with a `payload`.
  Relevant `event_msg` payload types: `task_started`, `task_complete`,
  `agent_message`, `user_message`, `token_count`, `patch_apply_end`.
- `~/.codex/session_index.jsonl` maps `id` → `thread_name` (title) →
  `updated_at`.

### Live processes
- Live `claude` / `codex` processes are enumerable. Their working directory is
  read via the native `libproc` API in Rust (macOS `lsof` fails on external
  `/Volumes` mounts, so `libproc` is used instead).

## Architecture

Tauri app. Rust backend organized as small, independently testable modules:

- **discovery** — enumerate Claude project folders and Codex rollout files
  (+ `session_index.jsonl`); group sessions into `Project`s keyed by `cwd`.
- **parser** — for each session JSONL, read only the **head** (first
  `system`/`session_meta` record) and a bounded **tail** (last N KB). Extract
  title, last-activity timestamp, last-event type, agent kind, git branch,
  model. Never parse an entire large file.
- **procs** — enumerate live `claude`/`codex` processes; map pid → cwd via
  `libproc`. Produces the set of cwds with a live agent.
- **status** — combine parser output, live-process set, and file mtime into a
  single `Status` per session (see model below).
- **launcher** — open the configured terminal at the session's cwd running the
  resume command (AppleScript).
- **watcher** — FS-watch both source dirs (`notify` crate) plus a periodic tick
  (~2s) for process/status refresh; emit updates to the frontend via Tauri
  events.

### Data model

```
Project {
  path: String,          // working directory
  name: String,          // basename of path
  branch: Option<String>,
  mounted: bool,         // false if external volume unmounted
  status: Status,        // aggregate (most urgent session wins)
  last_activity: DateTime,
  sessions: Vec<Session>,
}

Session {
  id: String,            // session/rollout uuid
  agent: Agent,          // Claude | Codex
  title: Option<String>,
  model: Option<String>,
  cwd: String,
  file_path: String,
  last_activity: DateTime,
  status: Status,
}

enum Agent  { Claude, Codex }
enum Status { Working, NeedsYou, Resumable, Offline }
```

### Status model

Per session, evaluated by the **status** module:

| Status | Condition |
|---|---|
| **Working** | live process at this cwd **and** ( trailing `task_started` (Codex) / trailing `tool_use` or non-`end_turn` last assistant (Claude) **or** file mtime < ~15s ) |
| **NeedsYou** | live process at this cwd **and** last event is `task_complete` (Codex) / assistant `stop_reason: end_turn` (Claude) |
| **Resumable** | no live process at this cwd, session has history |
| **Offline** | session's cwd is on an unmounted volume |

Project aggregate status = most urgent among its sessions
(NeedsYou > Working > Resumable > Offline).

### Launch / resume commands

- Claude, resume: `cd <cwd> && claude --resume <sessionId>`; fresh: `claude`
- Codex, resume: `cd <cwd> && codex resume <sessionId>`; fresh: `codex`
- Executed in the configured terminal via AppleScript (iTerm2 default: open new
  tab/window, `cd`, run command; Terminal.app fallback path).
- If the target cwd is unmounted, the tile is Offline and launch is disabled.

## Frontend (web UI)

- Grid of **project tiles**. Each tile: project name · agent icon(s)
  (Claude/Codex) · status badge & color · relative last-activity ·
  last-prompt/title snippet.
- Sort: **NeedsYou first, then most-recent activity**. NeedsYou tiles are
  visually highlighted (glow).
- Search/filter box over project name and title.
- Hover actions per tile: **Resume** · **Open folder** · **Copy path**.
- Window: small, always-on-top toggle, remembers position/size.

## Error handling

- Unreadable/partial JSONL records are skipped; a session with no parseable
  head is dropped, not fatal.
- Unmounted external volumes → `Offline`, never a crash.
- Large session files: bounded head+tail reads only.
- Missing terminal app → fall back to Terminal.app; surface a non-fatal error.
- Rust toolchain absent → handled once during project setup (rustup).

## Testing strategy

- **parser**: fixture JSONL for Claude and Codex → assert title, timestamp,
  agent, last-event extraction.
- **status**: truth-table tests over (live process present/absent) ×
  (last-event kind) × (mtime fresh/stale) → assert correct `Status`.
- **discovery**: build a temporary directory tree mimicking both layouts →
  assert projects/sessions are grouped by cwd correctly.
- **launcher**: unit-test command-string construction (not the AppleScript
  side-effect).

## Open questions / future (v2+)

- Native desktop notifications on transition to NeedsYou.
- Focusing an already-open terminal for a live session (vs. opening a new
  resume) — cross-terminal focus is non-trivial; deferred.
- Per-launch terminal choice.
- Optional physical macro-pad binding.
