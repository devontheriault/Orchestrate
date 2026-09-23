//! The live Agents: spawning them, resuming them, stopping them, and noticing
//! when they end. Each Turn is one `claude` process watched by a supervisor
//! task (see `turn`); this module keeps the map of which Agents have one.

mod events;
mod turn;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::domain::{new_id, new_session_id, Agent, AgentState, Id, Project, Resolution, Task};
use crate::error::{Error, Result};
use crate::{attachments, git, merging, paths, storage, worktree};

use events::{prompt_event, Emitter};
pub use events::{RuntimeEvent, NOTICE_EVENT_TYPE, PROMPT_EVENT_TYPE};
use turn::Continuity;

/// How long shutdown() waits for supervisor tasks to finish per Agent.
const SHUTDOWN_TIMEOUT_PER_AGENT: Duration = Duration::from_secs(5);

/// Every live Agent's handle, by id. Shared with the supervisors, which take
/// their Agent out when its Turn ends.
type LiveMap = Arc<RwLock<HashMap<Id, AgentHandle>>>;

/// The live-Agent map, held for writing. Taken across a busy check and the
/// launch that follows it, so the two can't interleave.
type Live<'a> = tokio::sync::RwLockWriteGuard<'a, HashMap<Id, AgentHandle>>;

/// Internal handle held by the runtime for one live Agent.
struct AgentHandle {
    agent: Agent,
    /// Notified when the user requests Stop.
    cancel: Arc<Notify>,
    /// The supervisor task; drops when the Agent exits.
    task: JoinHandle<()>,
}

/// Runtime container for all live Agents. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct AgentRuntime {
    inner: LiveMap,
    emitter: Emitter,
    /// The `claude` binary to invoke. Overridable in tests.
    claude_bin: Arc<String>,
    /// Whether a finished Turn spends a second `claude` call naming the Agent.
    /// Off in most tests, so the fake `claude` sees one invocation per Turn.
    naming: bool,
}

impl AgentRuntime {
    /// Construct a runtime and the receiver half of its event stream.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            emitter: Emitter::new(tx),
            claude_bin: Arc::new("claude".to_string()),
            naming: true,
        };
        (rt, rx)
    }

    /// Construct with a custom binary path (for testing with a fake claude).
    /// Naming is off: a test that counts invocations wants Turns only.
    #[cfg(test)]
    pub fn with_bin(bin: impl Into<String>) -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (rt, rx) = Self::new();
        let rt = Self {
            claude_bin: Arc::new(bin.into()),
            naming: false,
            ..rt
        };
        (rt, rx)
    }

    /// As `with_bin`, but the fake `claude` also plays the namer.
    #[cfg(test)]
    pub fn with_bin_naming(
        bin: impl Into<String>,
    ) -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (rt, rx) = Self::with_bin(bin);
        (Self { naming: true, ..rt }, rx)
    }

    /// Spawn a new Agent for the given Project and opening prompt, with any
    /// files attached to it, on the model and effort the user picked (`None`
    /// leaves either choice to Claude Code) and in the Permission Mode they
    /// picked (`None` is `DEFAULT_PERMISSION_MODE`). Returns the Agent record
    /// once the process is running and its meta file is on disk.
    pub async fn spawn(
        &self,
        project: &Project,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<Agent> {
        attachments::check(&attachments)?;
        let mut agent = new_agent(project, prompt, model, effort, permission_mode)?;
        agent.task.attachments = attachments;
        // Record the commit we branched from before the Agent can move HEAD, so
        // the diff view has a fixed base even if the Project advances later.
        agent.base_commit = git::head_commit(&project.path).await.ok();
        let from = agent.base_commit.clone().unwrap_or_else(|| "HEAD".into());
        self.start(project, &from, agent).await
    }

    /// Spawn a Resolver for a Merge of `conflicted` into `target` that hit
    /// conflicts in `files`: a new Agent on a branch cut from `conflicted`'s,
    /// told to merge `target` in and settle them. When its Turn Completes, the
    /// supervisor finishes the Merge (see [`merging::finish_resolution`]).
    ///
    /// Always YOLO, whatever the user last picked: a Resolver that may not write
    /// cannot resolve anything. Its Base is `target`'s tip, so its diff reads as
    /// what the finished Merge will bring into `target`.
    pub async fn spawn_resolver(
        &self,
        project: &Project,
        conflicted: &Agent,
        target: &str,
        files: &[String],
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<Agent> {
        let from = git::rev_parse(&project.path, &conflicted.branch).await?;
        let prompt = merging::resolver_prompt(conflicted, target, files);
        let mut agent = new_agent(
            project,
            prompt,
            model,
            effort,
            Some(crate::domain::DEFAULT_PERMISSION_MODE.into()),
        )?;
        agent.base_commit = Some(git::rev_parse(&project.path, target).await?);
        agent.resolves = Some(Resolution {
            agent_id: conflicted.id.clone(),
            target: target.to_owned(),
        });
        self.start(project, &from, agent).await
    }

    /// Cut the new Agent's Worktree from `from`, put its record on disk, and
    /// start its first Turn on its Task.
    async fn start(&self, project: &Project, from: &str, agent: Agent) -> Result<Agent> {
        worktree::create(&project.path, &agent.worktree_path, &agent.branch, from).await?;
        storage::save_agent(&agent)?;
        let Task {
            prompt,
            attachments,
        } = agent.task.clone();
        self.record_prompt(&agent, &prompt, &attachments);

        let mut live = self.inner.write().await;
        self.launch(agent, &prompt, &attachments, Continuity::Fresh, &mut live)
    }

    /// Continue an Agent's conversation with a follow-up prompt and any files
    /// attached to it: start a new
    /// `claude` in the Agent's existing Worktree, resuming its Session, and put
    /// the Agent back into `Running`.
    ///
    /// `model`, `effort` and `permission_mode` are the choices for this Turn
    /// onwards, and are recorded on the Agent — a user may start cheap and
    /// escalate mid-conversation, or plan first and then let the Agent loose.
    /// The caller always states them, so `None` means "let Claude Code pick",
    /// not "unchanged".
    ///
    /// Refused while the Agent is working, and for an Agent with no Session or
    /// no Worktree left to work in.
    pub async fn resume(
        &self,
        agent_id: &str,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<Agent> {
        attachments::check(&attachments)?;
        // Held from the busy check through the launch: two windows resuming the
        // same Agent at once must not both get past the check and put two
        // `claude`s to work in one Worktree.
        let mut live = self.inner.write().await;
        if live.contains_key(agent_id) {
            return Err(Error::AgentBusy(agent_id.to_string()));
        }
        let mut agent = storage::load_agent(agent_id)?;
        // Meta says Running but no supervisor owns it: a stale record this
        // launch never adopted. Refuse rather than run two `claude`s at once.
        if agent.state == AgentState::Running {
            return Err(Error::AgentBusy(agent_id.to_string()));
        }
        let why = |why: &str| Error::NotResumable {
            id: agent_id.to_string(),
            why: why.to_string(),
        };
        if agent.session_id.is_none() {
            return Err(why("it has no recorded session"));
        }
        if !agent.worktree_path.exists() {
            return Err(why("its worktree is gone"));
        }

        agent.turns += 1;
        agent.model = model;
        agent.effort = effort;
        agent.permission_mode = permission_mode;
        agent.state = AgentState::Running;
        agent.turn_started_at = Some(OffsetDateTime::now_utc());
        agent.exited_at = None;
        agent.exit_code = None;
        agent.fail_reason = None;
        storage::save_agent(&agent)?;
        self.record_prompt(&agent, &prompt, &attachments);
        self.emitter.announce(&agent);

        self.launch(agent, &prompt, &attachments, Continuity::Resumed, &mut live)
    }

    /// Append the user's prompt to the Agent's log and push it to the UI.
    fn record_prompt(&self, agent: &Agent, prompt: &str, attachments: &[PathBuf]) {
        self.emitter
            .record(&agent.id, prompt_event(prompt, attachments, agent.turns));
    }

    /// Start one Turn: spin up `claude` in the Agent's Worktree and hand the
    /// child to a supervisor. A Turn that cannot even be started is a Fail, so
    /// the Agent never sits in `Running` with no process behind it.
    fn launch(
        &self,
        agent: Agent,
        prompt: &str,
        attached: &[PathBuf],
        how: Continuity,
        live: &mut Live<'_>,
    ) -> Result<Agent> {
        let child = turn::command(&self.claude_bin, &agent, prompt, attached, how).spawn();

        let child = match child {
            Ok(child) => child,
            Err(source) => {
                let mut failed = agent.clone();
                failed.state = AgentState::Failed;
                failed.exited_at = Some(OffsetDateTime::now_utc());
                failed.fail_reason = Some(format!("could not start `claude`: {source}"));
                let _ = storage::save_agent(&failed);
                self.emitter.announce(&failed);
                return Err(Error::Io {
                    path: agent.worktree_path.clone(),
                    source,
                });
            }
        };

        let cancel = Arc::new(Notify::new());
        let task = tokio::spawn(turn::supervise(
            child,
            agent.clone(),
            cancel.clone(),
            self.emitter.clone(),
            self.inner.clone(),
            self.naming.then(|| self.claude_bin.clone()),
        ));

        live.insert(
            agent.id.clone(),
            AgentHandle {
                agent: agent.clone(),
                cancel,
                task,
            },
        );
        Ok(agent)
    }

    /// Request Stop for a working Agent. Returns when the supervisor has
    /// finished (meta persisted, state=Stopped). The Worktree is left alone so
    /// the Turn's work survives and the Agent can be Resumed; Reap is a
    /// separate, explicit action.
    pub async fn stop(&self, agent_id: &str) -> Result<()> {
        let handle = self.inner.write().await.remove(agent_id);
        match handle {
            Some(h) => {
                h.cancel.notify_one();
                let _ = h.task.await;
                Ok(())
            }
            None => Err(Error::AgentNotFound(agent_id.to_string())),
        }
    }

    /// Snapshot of currently-running Agents.
    pub async fn running(&self) -> Vec<Agent> {
        self.inner
            .read()
            .await
            .values()
            .map(|h| h.agent.clone())
            .collect()
    }

    /// Signal every live Agent to stop and wait (bounded) for their
    /// supervisors to finish. Called on app close. Worktrees are *not* reaped
    /// on shutdown — the next launch surfaces them as Orphaned via
    /// `adopt_orphans_on_launch`.
    pub async fn shutdown(&self) {
        let handles: Vec<AgentHandle> = {
            let mut inner = self.inner.write().await;
            inner.drain().map(|(_, h)| h).collect()
        };
        // Mark each Agent as Orphaned before sending cancel, so the supervisor
        // sees the Orphan path instead of Stop.
        for h in &handles {
            let mut agent = h.agent.clone();
            agent.state = AgentState::Orphaned;
            agent.exited_at = Some(OffsetDateTime::now_utc());
            let _ = storage::save_agent(&agent);
        }
        // Kill the processes.
        for h in &handles {
            h.cancel.notify_one();
        }
        for h in handles {
            let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT_PER_AGENT, h.task).await;
        }
    }
}

/// The record for an Agent about to be Spawned into `project`, before its
/// Worktree exists. Callers fill in where it branches from.
fn new_agent(
    project: &Project,
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
) -> Result<Agent> {
    let agent_id = new_id();
    let now = OffsetDateTime::now_utc();
    Ok(Agent {
        worktree_path: paths::worktrees_dir()?.join(&project.id).join(&agent_id),
        branch: format!("{}{agent_id}", crate::domain::AGENT_BRANCH_PREFIX),
        id: agent_id,
        project_id: project.id.clone(),
        task: Task {
            prompt,
            attachments: vec![],
        },
        state: AgentState::Running,
        base_commit: None,
        // Mint the Session ID rather than waiting to read it off the stream, so
        // the Agent is resumable even if it dies mid-first-line.
        session_id: Some(new_session_id()),
        model,
        effort,
        permission_mode,
        turns: 1,
        // Named at the end of its first Turn, once there is work to name.
        title: None,
        spawned_at: now,
        turn_started_at: Some(now),
        exited_at: None,
        exit_code: None,
        fail_reason: None,
        // Nothing has merged yet; a Merge records itself here when it does.
        merged_branch: None,
        merged_at: None,
        resolves: None,
    })
}

/// Scan every meta file on disk for Agents still marked `Running` and
/// transition them to `Orphaned` with `exited_at = now`. Called once at
/// startup. Returns the newly-adopted Orphans.
pub fn adopt_orphans_on_launch() -> Result<Vec<Agent>> {
    let all = storage::list_agents()?;
    let mut adopted = Vec::new();
    for mut agent in all {
        if agent.state == AgentState::Running {
            agent.state = AgentState::Orphaned;
            if agent.exited_at.is_none() {
                agent.exited_at = Some(OffsetDateTime::now_utc());
            }
            storage::save_agent(&agent)?;
            adopted.push(agent);
        }
    }
    Ok(adopted)
}
