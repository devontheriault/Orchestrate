use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Short random hex ID (8 chars) used for Agents and Projects.
pub type Id = String;

/// Every Agent's branch is named `cw/agent-<id>`. The prefix is how the rest of
/// the app tells an Agent's branch from one of the user's own — the Land picker
/// leaves these out, since landing one Agent onto another's branch is
/// multi-agent coordination rather than finishing a piece of work.
pub const AGENT_BRANCH_PREFIX: &str = "cw/agent-";

/// Generate a new 8-hex-char ID from 4 random bytes.
pub fn new_id() -> Id {
    let bytes: [u8; 4] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a random v4 UUID string, the form `claude --session-id` accepts.
/// We mint the Session's ID ourselves at Spawn so an Agent is resumable from
/// the moment it starts, even if it never emits a line of output.
pub fn new_session_id() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
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

/// The opening prompt the user hands an Agent at Spawn. Later Turns are
/// separate prompts, recorded in the Agent's log rather than here.
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

/// One Claude Code conversation, bound to one Worktree. An Agent outlives the
/// individual `claude` processes that run its Turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub id: Id,
    pub project_id: Id,
    pub task: Task,
    pub state: AgentState,
    pub worktree_path: PathBuf,
    pub branch: String,
    /// The commit the branch was cut from at Spawn. `None` for Agents recorded
    /// before base tracking existed; the diff falls back to a merge-base then.
    #[serde(default)]
    pub base_commit: Option<String>,
    /// The Claude Code session backing this Agent, used to Resume it. Minted at
    /// Spawn; refreshed if the stream ever reports a different one. `None` for
    /// Agents recorded before Sessions existed — those cannot be Resumed.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Which model the Agent's Turns run on, as `claude --model` takes it — an
    /// alias like `opus` or a full model name. `None` means we pass no `--model`
    /// at all and Claude Code picks, which is also what pre-model Agents get.
    #[serde(default)]
    pub model: Option<String>,
    /// How hard the Agent's Turns work, as `claude --effort` takes it: `low`,
    /// `medium`, `high`, `xhigh`, or `max`. `None` means we pass no `--effort`
    /// and Claude Code picks, which is also what pre-effort Agents get.
    #[serde(default)]
    pub effort: Option<String>,
    /// How many Turns have been started, including the opening one.
    #[serde(default = "one")]
    pub turns: u32,
    /// A short name for the Agent, written by Claude from the work so far.
    /// Re-written at the end of each of the first `TITLE_TURNS` Turns and then
    /// frozen, so an Agent settles on one name instead of drifting. `None`
    /// until the first Turn ends, and for Agents recorded before titles
    /// existed — readers fall back to the opening prompt.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub spawned_at: OffsetDateTime,
    /// When the current — or, once it has exited, the last — Turn began. Set at
    /// Spawn and again at every Resume, so "how long has this been working" is
    /// about the Turn rather than the Agent's whole life. `None` for Agents
    /// recorded before Turn timing existed; readers fall back to `spawned_at`.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub turn_started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub exited_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub fail_reason: Option<String>,
    /// The branch this Agent's work was last Landed onto, and when. `None`
    /// until a Land succeeds. Recorded like the Base — a fact about what
    /// happened to the Agent, not a state it is in, so a Landed Agent can
    /// still be Resumed, Committed, and Landed again.
    #[serde(default)]
    pub landed_branch: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub landed_at: Option<OffsetDateTime>,
}

fn one() -> u32 {
    1
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
                task: Task {
                    prompt: "do the thing".into(),
                },
                state,
                worktree_path: "/tmp/wt".into(),
                branch: "cw/agent-a3f9c1de".into(),
                base_commit: Some("deadbeef".into()),
                session_id: Some("0b8b3a6e-5d2f-4a71-8c3e-1f9d7a2b4c60".into()),
                model: Some("opus".into()),
                effort: Some("high".into()),
                turns: 3,
                title: Some("Wire up the thing".into()),
                spawned_at: datetime!(2026-09-20 14:00:00 UTC),
                turn_started_at: Some(datetime!(2026-09-20 14:02:00 UTC)),
                exited_at: Some(datetime!(2026-09-20 14:05:00 UTC)),
                exit_code: Some(0),
                fail_reason: None,
                landed_branch: None,
                landed_at: None,
            };
            let s = serde_json::to_string(&a).unwrap();
            let back: Agent = serde_json::from_str(&s).unwrap();
            assert_eq!(a, back);
        }
    }

    #[test]
    fn new_session_id_looks_like_a_v4_uuid() {
        let id = new_session_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert!(parts[2].starts_with('4'), "version nibble: {id}");
        assert!(
            matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
            "variant nibble: {id}"
        );
        assert_ne!(new_session_id(), new_session_id());
    }

    /// Meta files written before Sessions existed must still load: they simply
    /// have no Session, which the UI reads as "cannot be continued".
    #[test]
    fn agent_without_session_fields_still_deserializes() {
        let json = r#"{
            "id": "a3f9c1de",
            "project_id": "proj0001",
            "task": { "prompt": "old agent" },
            "state": "completed",
            "worktree_path": "/tmp/wt",
            "branch": "cw/agent-a3f9c1de",
            "spawned_at": "2026-09-20T14:00:00Z"
        }"#;
        let a: Agent = serde_json::from_str(json).unwrap();
        assert_eq!(a.session_id, None);
        assert_eq!(
            a.model, None,
            "no recorded model means Claude Code's own default"
        );
        assert_eq!(
            a.effort, None,
            "no recorded effort means Claude Code's own default"
        );
        assert_eq!(a.turns, 1, "a pre-Session agent had exactly one turn");
        assert_eq!(
            a.title, None,
            "no recorded title means readers fall back to the prompt"
        );
        assert_eq!(
            a.turn_started_at, None,
            "no recorded turn start means readers fall back to spawned_at"
        );
    }

    #[test]
    fn agent_state_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&AgentState::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&AgentState::Orphaned).unwrap(),
            "\"orphaned\""
        );
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
