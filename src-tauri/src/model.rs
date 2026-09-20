use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Short random hex ID (8 chars) used for Agents and Projects.
pub type Id = String;

/// Generate a new 8-hex-char ID from 4 random bytes.
pub fn new_id() -> Id {
    let bytes: [u8; 4] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A local Git repository the user has registered with the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub name: String,
    pub path: PathBuf,
    #[serde(with = "time::serde::rfc3339")]
    pub added_at: OffsetDateTime,
}

/// The prompt the user hands an Agent at spawn time. V1 is one-shot: fully
/// specified up front, no follow-ups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub prompt: String,
}

/// Lifecycle state of an Agent. See CONTEXT.md for the semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Running,
    Completed,
    Failed,
    Stopped,
    Orphaned,
}

/// One `claude` process, bound to one Worktree, running one Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub id: Id,
    pub project_id: Id,
    pub task: Task,
    pub state: AgentState,
    pub worktree_path: PathBuf,
    pub branch: String,
    #[serde(with = "time::serde::rfc3339")]
    pub spawned_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub exited_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub fail_reason: Option<String>,
}

/// One line in an Agent's JSONL log: a wall-clock timestamp plus the raw
/// stream-json event as we received it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub event: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn new_id_is_8_hex_chars() {
        let id = new_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn project_roundtrips() {
        let p = Project {
            id: "abcd1234".into(),
            name: "MyProj".into(),
            path: "/home/dev/proj".into(),
            added_at: datetime!(2026-09-20 14:00:00 UTC),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn agent_roundtrips_with_all_states() {
        for state in [
            AgentState::Running,
            AgentState::Completed,
            AgentState::Failed,
            AgentState::Stopped,
            AgentState::Orphaned,
        ] {
            let a = Agent {
                id: "a3f9c1de".into(),
                project_id: "proj0001".into(),
                task: Task { prompt: "do the thing".into() },
                state,
                worktree_path: "/tmp/wt".into(),
                branch: "cw/agent-a3f9c1de".into(),
                spawned_at: datetime!(2026-09-20 14:00:00 UTC),
                exited_at: Some(datetime!(2026-09-20 14:05:00 UTC)),
                exit_code: Some(0),
                fail_reason: None,
            };
            let s = serde_json::to_string(&a).unwrap();
            let back: Agent = serde_json::from_str(&s).unwrap();
            assert_eq!(a, back);
        }
    }

    #[test]
    fn agent_state_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&AgentState::Running).unwrap(), "\"running\"");
        assert_eq!(serde_json::to_string(&AgentState::Orphaned).unwrap(), "\"orphaned\"");
    }

    #[test]
    fn agent_event_roundtrips() {
        let e = AgentEvent {
            ts: datetime!(2026-09-20 14:00:00 UTC),
            event: serde_json::json!({"type": "assistant", "text": "hi"}),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: AgentEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e.event, back.event);
        assert_eq!(e.ts, back.ts);
    }
}
