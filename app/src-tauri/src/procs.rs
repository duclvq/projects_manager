use sysinfo::System;

/// Number of live `claude` / `codex` agent processes.
///
/// macOS blocks reading another process's working directory without root, so we
/// cannot map a process to a project. We can, however, reliably count how many
/// agents are running (the process name is readable). Combined with per-session
/// file recency (see `status`), this tells us whether anything is live at all.
pub fn live_agent_count() -> usize {
    let mut sys = System::new();
    sys.refresh_processes();

    sys.processes()
        .values()
        .filter(|p| {
            let name = p.name();
            name == "claude" || name == "codex"
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_without_panicking() {
        // Environment-dependent; just assert the call succeeds.
        let _ = live_agent_count();
    }
}
