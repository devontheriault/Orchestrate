//! What the runtime tells the rest of the app, and the events the app writes
//! into an Agent's log itself.

use std::path::PathBuf;

use time::OffsetDateTime;
use tokio::sync::mpsc;

use crate::domain::{Agent, AgentEvent, Id};
use crate::storage;

/// Event type for a prompt the *user* sent. Our own invention — `claude` never
/// emits it — so the transcript can show both sides of the conversation.
pub const PROMPT_EVENT_TYPE: &str = "cw_prompt";

/// Record a user prompt as a log event, so a Turn's question sits above its
/// answer in the output pane and survives a reload.
/// The prompt is the user's own text, with the attachments beside it rather
/// than folded in, so the transcript can show them as files.
pub(super) fn prompt_event(prompt: &str, attachments: &[PathBuf], turn: u32) -> AgentEvent {
    let mut event = serde_json::json!({
        "type": PROMPT_EVENT_TYPE,
        "prompt": prompt,
        "turn": turn,
    });
    if !attachments.is_empty() {
        event["attachments"] = serde_json::json!(attachments);
    }
    AgentEvent {
        ts: OffsetDateTime::now_utc(),
        event,
    }
}

/// Event type for something the app itself has to say in an Agent's
/// transcript — like the prompt, never emitted by `claude`.
pub const NOTICE_EVENT_TYPE: &str = "cw_notice";

/// Record a line the app wants the user to read in an Agent's transcript.
pub(super) fn notice_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: OffsetDateTime::now_utc(),
        event: serde_json::json!({
            "type": NOTICE_EVENT_TYPE,
            "text": text,
        }),
    }
}

/// Events emitted by the runtime for consumers (Tauri IPC in Phase 3).
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// One line from a running Agent's stdout, parsed and timestamped.
    AgentEvent { agent_id: Id, event: AgentEvent },
    /// The Agent's persisted state changed (state, exit_code, exited_at).
    /// Boxed: an `Agent` dwarfs an `AgentEvent`, and every event sent down the
    /// channel would otherwise pay for the larger variant.
    StateChanged { agent_id: Id, agent: Box<Agent> },
}

/// The sending half of the runtime's event stream. Everything the runtime says
/// goes through one of its two methods, so a transcript line is always on disk
/// before the UI hears of it.
#[derive(Clone)]
pub(super) struct Emitter(mpsc::UnboundedSender<RuntimeEvent>);

impl Emitter {
    pub(super) fn new(tx: mpsc::UnboundedSender<RuntimeEvent>) -> Self {
        Self(tx)
    }

    /// Append an event to the Agent's log, then push it to the UI, both
    /// without the weight the transcript never shows (see [`storage::slim`]).
    /// An event no window shows at all is only logged (see [`storage::unseen`]).
    pub(super) fn record(&self, agent_id: &str, mut event: AgentEvent) {
        storage::slim(&mut event.event);
        let _ = storage::append_event(agent_id, &event);
        if storage::unseen(&event.event) {
            return;
        }
        let _ = self.0.send(RuntimeEvent::AgentEvent {
            agent_id: agent_id.to_owned(),
            event,
        });
    }

    /// Tell consumers the Agent's persisted record moved.
    pub(super) fn announce(&self, agent: &Agent) {
        let _ = self.0.send(RuntimeEvent::StateChanged {
            agent_id: agent.id.clone(),
            agent: Box::new(agent.clone()),
        });
    }
}
