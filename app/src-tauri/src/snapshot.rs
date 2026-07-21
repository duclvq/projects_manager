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
