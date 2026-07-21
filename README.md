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
