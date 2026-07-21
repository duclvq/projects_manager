# Session changes — 2026-07-21

This document records the complete Projects Manager work completed in the
2026-07-21 development session.

## Behavior changes

### Codex opens with Full access

Every fresh or resumed Codex session launched from the panel now includes:

```text
--dangerously-bypass-approvals-and-sandbox
```

This is the CLI equivalent of opening `/permissions` and selecting **Full
access**. It sets an unrestricted sandbox and disables approval prompts for that
invocation. The setting is scoped to Codex sessions opened by Projects Manager;
the user's global Codex configuration is not modified.

This mode is intentionally dangerous. Only launch Codex from the panel for
repositories and instructions you trust.

### Usage dashboard correctness

The previous Claude gauge was removed. It incorrectly estimated a weekly
remaining percentage by dividing locally counted tokens by a hard-coded 40M
token budget. The audit found two concrete problems:

- Claude plan limits are account-wide five-hour and fixed weekly windows, not a
  rolling seven-day local token total.
- 1,218 local usage records represented only 539 unique message IDs, so the old
  implementation counted 679 repeated records and inflated its token/cost
  totals.

Claude usage now comes from the authenticated Claude Code `/usage` command. A
short-lived `expect` TTY launches `claude --no-chrome`, runs `/usage`, captures
the current-session and current-week percentages/reset labels, then exits. It
does not send a prompt to a model. The latest successful result is cached; if a
later refresh fails, the UI retains it and marks it `stale`.

Claude executable discovery checks, in order:

1. `CLAUDE_BIN` when set to an existing file.
2. `~/.local/bin/claude`.
3. `~/.claude/local/claude`.
4. `command -v claude` in a login `zsh`.

The collector requires macOS `/usr/bin/expect` and an authenticated Claude CLI.
It has a bounded timeout and runs from the system temporary directory so its
short-lived process is not associated with a managed project.

Codex usage now supports:

- Current thread context remaining, calculated from
  `last_token_usage.total_tokens` and `model_context_window`.
- Primary and optional secondary plan rate-limit windows.
- Accurate reset labels for every reported plan window.

Usage is fetched once on frontend load. The backend emits a refreshed snapshot
every 60 seconds; the previous 15-second usage polling loop was removed.

### Window and visual behavior

- The panel starts with **On top** disabled. The button still enables it for the
  current run.
- CODEX/CLAUDE labels and their plan badges use an explicit vertical stack with
  a 4px gap, preventing the overlap visible in the previous UI.
- Claude displays separate session and weekly gauges with a `live` or `stale`
  source badge.
- Codex displays context and plan-limit gauges as separate compact rows.

## Files changed

- `app/src-tauri/src/launcher.rs` — Codex Full access launch flag and tests.
- `app/src-tauri/src/usage.rs` — authoritative Claude `/usage` collector,
  parser/cache, Codex context and multi-window parsing, and tests.
- `app/src-tauri/src/lib.rs` — one-minute usage refresh cadence.
- `app/src/main.js` — multi-row Codex and Claude usage rendering.
- `app/src/styles.css` — compact usage rows and non-overlapping plan badges.
- `app/src/index.html` — unpinned initial button styling.
- `app/src-tauri/tauri.conf.json` — `alwaysOnTop: false` default.
- `README.md` — runtime behavior, usage sources, and safety notes.

## Verification

The implementation was checked with:

```text
cargo test
node --check app/src/main.js
jq empty app/src-tauri/tauri.conf.json
git diff --check
npm run tauri build
```

Results at handoff:

- 25 Rust tests passed; 0 failed.
- JavaScript syntax, Tauri JSON, and whitespace checks passed.
- The live Tauri panel was visually inspected at its configured 440px width.
- Claude values in the panel matched the same `/usage` invocation: session
  exhausted and 24% weekly used at the time of verification.
- The macOS application and arm64 DMG built successfully.

Build outputs:

```text
app/src-tauri/target/release/bundle/macos/Projects Manager.app
app/src-tauri/target/release/bundle/dmg/Projects Manager_0.1.0_aarch64.dmg
```

## Known limitations

- Claude does not expose `/usage` as a documented noninteractive JSON command,
  so the collector parses its TUI text. Unit fixtures cover the expected output,
  and failures fall back to the last successful snapshot, but a future Claude UI
  wording change may require a parser update.
- Only Claude's current-session and all-model weekly windows are displayed.
  Extra-usage credit spending and model-scoped weekly limits are not yet shown.
- Codex displays only the plan windows present in its latest local rate-limit
  snapshot.
