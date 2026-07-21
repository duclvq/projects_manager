use crate::model::{LastEvent, Session, Status};
use chrono::{DateTime, Utc};
use std::path::Path;

/// A session is considered "actively working" if its file was written this
/// recently (the agent is streaming into it right now).
const WORKING_SECS: i64 = 90;
/// A finished turn this recent — with an agent process alive — is treated as
/// "needs you" (agent finished, terminal likely still open awaiting input).
const NEEDS_YOU_SECS: i64 = 600;

/// Derive a session's status from mount state, how recently its file changed,
/// its last turn, and whether ANY agent process is running.
///
/// macOS won't let us map a process to a project without root, so we can't know
/// which project a live agent belongs to. Instead: if no agent is running, every
/// session is resumable; if agents are running, recent file activity marks the
/// session working/needs-you.
pub fn apply(session: &mut Session, live_count: usize, now: DateTime<Utc>) {
    let cwd = Path::new(&session.cwd);
    if !cwd.exists() {
        session.status = Status::Offline;
        return;
    }

    if live_count == 0 {
        session.status = Status::Resumable;
        return;
    }

    let age = (now - session.last_activity).num_seconds();
    session.status = match session.last_event {
        LastEvent::Working | LastEvent::Unknown if age < WORKING_SECS => Status::Working,
        LastEvent::Finished if age < NEEDS_YOU_SECS => Status::NeedsYou,
        _ => Status::Resumable,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Agent, LastEvent, Session, Status};
    use chrono::{Duration, Utc};

    fn make(cwd: &str, ev: LastEvent, age_secs: i64) -> Session {
        Session {
            id: "s".into(),
            agent: Agent::Claude,
            title: None,
            model: None,
            cwd: cwd.to_string(),
            branch: None,
            file_path: "/tmp".into(),
            last_activity: Utc::now() - Duration::seconds(age_secs),
            last_event: ev,
            status: Status::Resumable,
        }
    }

    #[test]
    fn unmounted_path_is_offline() {
        let mut s = make("/no/such/path/xyz123", LastEvent::Finished, 0);
        apply(&mut s, 1, Utc::now());
        assert_eq!(s.status, Status::Offline);
    }

    #[test]
    fn no_live_agents_means_resumable() {
        let mut s = make("/tmp", LastEvent::Working, 5);
        apply(&mut s, 0, Utc::now());
        assert_eq!(s.status, Status::Resumable);
    }

    #[test]
    fn fresh_working_session_is_working() {
        let mut s = make("/tmp", LastEvent::Working, 10);
        apply(&mut s, 1, Utc::now());
        assert_eq!(s.status, Status::Working);
    }

    #[test]
    fn fresh_finished_session_is_needs_you() {
        let mut s = make("/tmp", LastEvent::Finished, 30);
        apply(&mut s, 1, Utc::now());
        assert_eq!(s.status, Status::NeedsYou);
    }

    #[test]
    fn recently_finished_session_is_needs_you() {
        let mut s = make("/tmp", LastEvent::Finished, 300);
        apply(&mut s, 1, Utc::now());
        assert_eq!(s.status, Status::NeedsYou);
    }

    #[test]
    fn stale_session_is_resumable_even_with_live_agents() {
        let mut s = make("/tmp", LastEvent::Working, 5000);
        apply(&mut s, 2, Utc::now());
        assert_eq!(s.status, Status::Resumable);
    }
}
