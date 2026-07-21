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
