# Projects Manager

Floating macOS panel that lists your recent Claude Code and Codex projects,
shows each one's live status, and resumes a session in iTerm2 with one click.
The panel starts unpinned; use **On top** when you want it to float above other
windows.

Codex sessions launched from the panel start in **Full access** mode (the CLI
equivalent of selecting Full access from `/permissions`). This disables Codex
approval prompts and sandbox restrictions for that launched session; use the
Codex buttons only for projects you trust.

## Usage dashboard

- **Codex context** is calculated from the newest session's reported token count
  and model context window.
- **Codex plan limits** come from the newest rate-limit snapshot written by
  Codex. The UI supports both primary and secondary windows when provided.
- **Claude limits** come from Claude Code's authenticated `/usage` command—not
  estimated token totals. The panel shows the five-hour session allowance and
  weekly allowance with their reset times.
- Usage refreshes once when the panel loads and every **60 seconds** afterward.
  If Claude refresh fails after a successful fetch, the last snapshot remains
  visible with a `stale` badge.

Claude usage collection requires an authenticated `claude` CLI and macOS's
`/usr/bin/expect`. It runs `/usage` in a short-lived TTY without sending a model
prompt or consuming a model turn. Set `CLAUDE_BIN` to an absolute executable
path if Claude is installed somewhere nonstandard.

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
- Claude Code's authenticated `/usage` screen (session and weekly plan limits)

See [docs/2026-07-21-session-changes.md](docs/2026-07-21-session-changes.md)
for the implementation and verification record for the latest changes.
