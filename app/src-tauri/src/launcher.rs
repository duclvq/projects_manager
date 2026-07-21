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
