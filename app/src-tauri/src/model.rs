use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    NeedsYou,
    Working,
    Resumable,
    Offline,
}

impl Status {
    /// Lower rank = more urgent. Used for sorting tiles and aggregating projects.
    pub fn rank(&self) -> u8 {
        match self {
            Status::NeedsYou => 0,
            Status::Working => 1,
            Status::Resumable => 2,
            Status::Offline => 3,
        }
    }
}

/// Classification of a session's last turn, derived by the parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEvent {
    Working,  // mid-turn (agent still producing)
    Finished, // turn complete (waiting for the user)
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub title: Option<String>,
    pub model: Option<String>,
    pub cwd: String,
    pub branch: Option<String>,
    pub file_path: String,
    pub last_activity: DateTime<Utc>,
    #[serde(skip)]
    pub last_event: LastEvent,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub path: String,
    pub name: String,
    pub branch: Option<String>,
    pub mounted: bool,
    pub status: Status,
    pub last_activity: DateTime<Utc>,
    pub sessions: Vec<Session>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_rank_orders_needs_you_first() {
        assert!(Status::NeedsYou.rank() < Status::Working.rank());
        assert!(Status::Working.rank() < Status::Resumable.rank());
        assert!(Status::Resumable.rank() < Status::Offline.rank());
    }

    #[test]
    fn status_serializes_snake_case() {
        let j = serde_json::to_string(&Status::NeedsYou).unwrap();
        assert_eq!(j, "\"needs_you\"");
    }
}
