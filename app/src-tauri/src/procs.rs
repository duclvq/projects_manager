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
            if let Some(cwd) = process.cwd() {
                if !cwd.as_os_str().is_empty() {
                    set.insert(cwd.to_path_buf());
                }
            }
        }
    }
    set
}

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
