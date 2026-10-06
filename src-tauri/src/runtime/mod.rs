//! The live Agents: spawning them, resuming them, stopping them, and noticing
//! when they end. Each Turn is one `claude` process watched by a supervisor
//! task (see `turn`); this module keeps the map of which Agents have one.

mod events;
pub mod mcp;
mod turn;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use tokio::sync::{mpsc, watch, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::domain::{
    new_id, new_session_id, Agent, AgentOptions, AgentState, Id, Project, QueuedMessage,
    Resolution, Task, PLAN_PERMISSION_MODE,
};
use crate::error::{Error, Result};
use crate::handoff::{self, Handoff};
use crate::{attachments, git, merging, paths, storage, worktree};

use events::{notice_event, prompt_event, Emitter};
pub use events::{RuntimeEvent, NOTICE_EVENT_TYPE, PROMPT_EVENT_TYPE};
use turn::Continuity;

/// How long shutdown() waits for supervisor tasks to finish per Agent.
const SHUTDOWN_TIMEOUT_PER_AGENT: Duration = Duration::from_secs(5);

/// How many Helpers one Lead may have working at once (ADR 0019). Each is a
/// `claude` spending against the same account, and a model asked to work in
/// parallel will happily start thirty.
pub const MAX_WORKING_HELPERS: usize = 8;

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
    /// Turns `true` once the supervisor has recorded how the Turn ended and
    /// taken the Agent out of the live map.
    settled: watch::Receiver<bool>,
}

/// Runtime container for all live Agents. Cheap to clone (Arc inside).
#[derive(Clone)]
pub struct AgentRuntime {
    inner: LiveMap,
    emitter: Emitter,
    /// The `claude` binary to invoke. Overridable in tests.
    claude_bin: Arc<String>,
    /// Whether each Turn is handed the app's MCP server, so its Agent can
    /// reach the Spaces (see [`crate::mcp::turn_args`]). Off in tests: a fake
    /// `claude` has no use for it.
    mcp: bool,
    /// Whether a Spawn and a finished Turn spend a second `claude` call naming
    /// the Agent.
    /// Off in most tests, so the fake `claude` sees one invocation per Turn.
    naming: bool,
    /// Held from counting a Lead's working Helpers until the new one is
    /// working too, so a Lead Spawning several at once can't pass the limit.
    spawning_helper: Arc<tokio::sync::Mutex<()>>,
    /// Set once the Host has decided to exit (see [`Self::close_if_idle`]).
    /// Read and written only under the live-map lock, so no Turn can start
    /// between the check that the Host is idle and the exit that follows.
    closed: Arc<AtomicBool>,
}

impl AgentRuntime {
    /// Construct a runtime and the receiver half of its event stream.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            emitter: Emitter::new(tx),
            claude_bin: Arc::new("claude".to_string()),
            mcp: true,
            naming: true,
            spawning_helper: Arc::default(),
            closed: Arc::default(),
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
            mcp: false,
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

    /// The `claude` binary every Turn runs, for anything else that has to ask
    /// the same install a question.
    pub fn claude_bin(&self) -> &str {
        &self.claude_bin
    }

    /// Spawn a new Agent for the given Project and opening prompt, with any
    /// files attached to it, on the model and effort the user picked (`None`
    /// leaves either choice to Claude Code) and in the Permission Mode they
    /// picked (`None` is `DEFAULT_PERMISSION_MODE`), with the `options` the
    /// user set for it. Returns the Agent record once the process is running
    /// and its meta file is on disk.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        &self,
        project: &Project,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        options: AgentOptions,
    ) -> Result<Agent> {
        let agent = opening(
            project,
            prompt,
            attachments,
            model,
            effort,
            permission_mode,
            options,
        )?;
        self.spawn_opened(project, agent).await
    }

    /// Spawn an Agent whose Task holds mail (ADR 0018): a Spawn like any other,
    /// but Mail-locked from its first Turn, so it reads and suggests and can do
    /// nothing else for the rest of its life. There is no Mode to pick.
    pub async fn spawn_with_mail(
        &self,
        project: &Project,
        prompt: String,
        model: Option<String>,
        effort: Option<String>,
        options: AgentOptions,
    ) -> Result<Agent> {
        let mut agent = opening(project, prompt, vec![], model, effort, None, options)?;
        agent.read_mail = true;
        agent.permission_mode = Some(PLAN_PERMISSION_MODE.into());
        self.spawn_opened(project, agent).await
    }

    /// Cut the Worktree for a freshly opened Agent from the Project's tip and
    /// start its first Turn.
    async fn spawn_opened(&self, project: &Project, mut agent: Agent) -> Result<Agent> {
        // Record the commit we branched from before the Agent can move HEAD, so
        // the diff view has a fixed base even if the Project advances later.
        // Brought up to date with the remote first, where there is one.
        let start = git::spawn_start(&project.path).await.ok();
        agent.base_commit = start.as_ref().map(|s| s.commit.clone());
        let from = agent.base_commit.clone().unwrap_or_else(|| "HEAD".into());
        let agent = self.start(project, &from, agent).await?;
        if let Some(note) = start.and_then(|s| s.note) {
            self.emitter.record(&agent.id, notice_event(&note));
        }
        Ok(agent)
    }

    /// Spawn an Agent that picks up another Host's Agent's work from
    /// `handoff` (see [`handoff`]): its branch is cut from the one the other
    /// Host put on the remote, and its Task is the brief followed by `prompt`.
    /// Otherwise a Spawn like any other, taking the same picks.
    ///
    /// Its Base is the other Agent's where this Host has it in the fetched
    /// history, so its diff reads as the whole of the work; else the tip. It
    /// goes by the other Agent's Title until its own first Turn names it.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_handoff(
        &self,
        project: &Project,
        handoff: &Handoff,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        options: AgentOptions,
    ) -> Result<Agent> {
        let mut agent = opening(
            project,
            handoff::task(handoff, &prompt),
            attachments,
            model,
            effort,
            permission_mode,
            options,
        )?;
        let tip = git::fetch_published(&project.path, &handoff.branch).await?;
        let base = match &handoff.base_commit {
            Some(b)
                if git::is_ancestor(&project.path, b, &tip)
                    .await
                    .unwrap_or(false) =>
            {
                b.clone()
            }
            _ => tip.clone(),
        };
        agent.base_commit = Some(base);
        agent.title = handoff.title.clone();
        let agent = self.start(project, &tip, agent).await?;
        git::unpublish(&project.path, &handoff.branch).await;
        Ok(agent)
    }

    /// Spawn a Resolver for a Merge of `conflicted` into `target` that hit
    /// conflicts in `files`: a new Agent on a branch cut from `conflicted`'s,
    /// told to merge `target` in and settle them. When its Turn Completes, the
    /// supervisor finishes the Merge (see [`merging::finish_resolution`]),
    /// pushing `target` after if `push`, as the Merge that conflicted was to.
    ///
    /// Always YOLO, whatever the user last picked: a Resolver that may not write
    /// cannot resolve anything. Its Base is `target`'s tip, so its diff reads as
    /// what the finished Merge will bring into `target`.
    ///
    /// Refused for a Mail-locked Agent (ADR 0018), whose Task the Resolver's
    /// prompt would quote. It writes nothing, so it never has a Merge to resolve.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_resolver(
        &self,
        project: &Project,
        conflicted: &Agent,
        target: &str,
        files: &[String],
        push: bool,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<Agent> {
        if conflicted.read_mail {
            return Err(Error::MailLocked { action: "merge" });
        }
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
            push,
        });
        self.start(project, &from, agent).await
    }

    /// Spawn a Helper for `lead` on `prompt` (ADR 0019): an Agent in the
    /// Lead's Project whose branch and Base are the Lead's current commit, so
    /// it starts from what the Lead has committed and its diff is only its own
    /// work. It runs in the Lead's Mode and with its Options, on `model` and
    /// `effort` or else the Lead's, so it never runs looser than its Lead.
    ///
    /// Refused for a Mail-locked Lead, whose Task for it could carry the mail
    /// (ADR 0018); for a Helper, since Helpers don't lead; and for a Lead with
    /// [`MAX_WORKING_HELPERS`] already working.
    pub async fn spawn_helper(
        &self,
        project: &Project,
        lead: &Agent,
        prompt: String,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<Agent> {
        let refuse = |why: String| Err(Error::CannotLead(why));
        if lead.read_mail {
            return refuse(
                "this agent was handed mail, and what it writes for a Helper could carry it".into(),
            );
        }
        if lead.lead_id.is_some() {
            return refuse("a Helper can't spawn Helpers of its own".into());
        }
        let _one_at_a_time = self.spawning_helper.lock().await;
        let working = self
            .inner
            .read()
            .await
            .values()
            .filter(|h| h.agent.lead_id.as_deref() == Some(&lead.id))
            .count();
        if working >= MAX_WORKING_HELPERS {
            return refuse(format!(
                "{working} Helpers are already working, the most one Lead may have; \
                 wait for some to finish"
            ));
        }
        let from = git::rev_parse(&project.path, &lead.branch).await?;
        let mut agent = new_agent(
            project,
            prompt,
            model.or_else(|| lead.model.clone()),
            effort.or_else(|| lead.effort.clone()),
            lead.permission_mode.clone(),
        )?;
        agent.options = lead.options.clone();
        agent.base_commit = Some(from.clone());
        agent.lead_id = Some(lead.id.clone());
        self.start(project, &from, agent).await
    }

    /// Cut the new Agent's Worktree from `from`, put its record on disk, and
    /// start its first Turn on its Task.
    async fn start(&self, project: &Project, from: &str, agent: Agent) -> Result<Agent> {
        // Checked again under the lock in `launch`; this only saves cutting a
        // Worktree for a Turn that could not start.
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::HostClosing);
        }
        worktree::create(&project.path, &agent.worktree_path, &agent.branch, from).await?;
        storage::save_agent(&agent)?;
        let Task {
            prompt,
            attachments,
        } = agent.task.clone();
        self.record_prompt(&agent, &prompt, &attachments);

        let mut live = self.inner.write().await;
        let agent = self.launch(agent, &prompt, &attachments, Continuity::Fresh, &mut live)?;
        // Every window hears of a new Agent, not only the one that spawned it:
        // on another machine, the spawner may not be a window here at all.
        self.emitter.announce(&agent);
        // Named off its Task now, rather than when the first Turn ends, so the
        // sidebar has a real name within seconds. A handed-off Agent already
        // goes by the other Agent's Title.
        if self.naming && agent.title.is_none() {
            tokio::spawn(turn::name_agent(
                self.claude_bin.clone(),
                agent.clone(),
                None,
                self.emitter.clone(),
            ));
        }
        Ok(agent)
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
        let agent = self.free(agent_id, &live)?;
        let message = QueuedMessage {
            id: new_id(),
            prompt,
            attachments,
            model,
            effort,
            permission_mode,
        };
        self.begin_turn(agent, message, &mut live)
    }

    /// Say something to an Agent: a new Turn if it is free, the end of its
    /// Queue if it is working. The one place that choice is made, under the
    /// lock a Turn starts under, so a window never has to guess — and two
    /// windows sending at once can't both start a Turn in one Worktree.
    pub async fn send(&self, agent_id: &str, message: QueuedMessage) -> Result<Agent> {
        attachments::check(&message.attachments)?;
        let mut live = self.inner.write().await;
        if live.contains_key(agent_id) {
            return self.edit_locked(agent_id, |a| a.queue.push(message), &mut live);
        }
        let agent = self.free(agent_id, &live)?;
        self.begin_turn(agent, message, &mut live)
    }

    /// Send the head of an Agent's Queue now: the user's go-ahead for a Queue
    /// that a Stop or a Fail held.
    pub async fn send_next(&self, agent_id: &str) -> Result<Agent> {
        let mut live = self.inner.write().await;
        let agent = self.free(agent_id, &live)?;
        self.next_turn(agent, &mut live)
    }

    /// Start a Turn on the first message in a free Agent's Queue. Called with
    /// the live map held, by [`Self::send_next`] and by a supervisor whose Turn
    /// has just Completed. The message stays queued if it can't be sent.
    fn next_turn(&self, mut agent: Agent, live: &mut Live<'_>) -> Result<Agent> {
        let Some(next) = agent.queue.first() else {
            return Ok(agent);
        };
        attachments::check(&next.attachments)?;
        let message = agent.queue.remove(0);
        self.begin_turn(agent, message, live)
    }

    /// The record of an Agent that may start a Turn now, or why it may not:
    /// the Host is closing, it is already working, or it has no Session or no
    /// Worktree left to work in.
    fn free(&self, agent_id: &str, live: &Live<'_>) -> Result<Agent> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::HostClosing);
        }
        if live.contains_key(agent_id) {
            return Err(Error::AgentBusy(agent_id.to_string()));
        }
        let agent = storage::load_agent(agent_id)?;
        // Meta says Running but no supervisor owns it: a stale record this
        // Host never adopted. Refuse rather than run two `claude`s at once.
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
        Ok(agent)
    }

    /// Put a free Agent to work on `message`, on the picks it carries.
    fn begin_turn(
        &self,
        mut agent: Agent,
        message: QueuedMessage,
        live: &mut Live<'_>,
    ) -> Result<Agent> {
        agent.turns += 1;
        agent.model = message.model;
        agent.effort = message.effort;
        // Whatever the message was sent or queued under, a Mail-locked Agent
        // stays in `plan` (ADR 0018).
        agent.permission_mode = if agent.read_mail {
            Some(PLAN_PERMISSION_MODE.into())
        } else {
            message.permission_mode
        };
        agent.state = AgentState::Running;
        agent.turn_started_at = Some(OffsetDateTime::now_utc());
        agent.exited_at = None;
        agent.exit_code = None;
        agent.fail_reason = None;
        storage::save_agent(&agent)?;
        self.record_prompt(&agent, &message.prompt, &message.attachments);
        self.emitter.announce(&agent);

        self.launch(
            agent,
            &message.prompt,
            &message.attachments,
            Continuity::Resumed,
            live,
        )
    }

    /// Change what the user may change about an Agent at any moment — its
    /// Title, Tag, options and Queue — whether or not it is working, and
    /// announce the result. Options take effect from the next Turn.
    ///
    /// A working Agent's supervisor holds its own copy of the record and saves
    /// it when the Turn ends. It carries these fields over from disk as it does
    /// (see `Agent::keep_edits`), under the same lock held here, so the edit
    /// survives; the live handle is updated too, for a shutdown that saves it.
    pub async fn edit(&self, agent_id: &str, change: impl FnOnce(&mut Agent)) -> Result<Agent> {
        let mut live = self.inner.write().await;
        self.edit_locked(agent_id, change, &mut live)
    }

    fn edit_locked(
        &self,
        agent_id: &str,
        change: impl FnOnce(&mut Agent),
        live: &mut Live<'_>,
    ) -> Result<Agent> {
        let mut agent = storage::load_agent(agent_id)?;
        change(&mut agent);
        storage::save_agent(&agent)?;
        if let Some(handle) = live.get_mut(agent_id) {
            handle.agent.keep_edits(&agent);
        }
        self.emitter.announce(&agent);
        Ok(agent)
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
        let fail = |why: String| {
            let mut failed = agent.clone();
            failed.state = AgentState::Failed;
            failed.exited_at = Some(OffsetDateTime::now_utc());
            failed.fail_reason = Some(why);
            let _ = storage::save_agent(&failed);
            self.emitter.announce(&failed);
        };
        // A Spawn that got past the early check in `start` as the Host closed.
        if self.closed.load(Ordering::SeqCst) {
            fail(Error::HostClosing.to_string());
            return Err(Error::HostClosing);
        }

        let child =
            turn::command(&self.claude_bin, self.mcp, &agent, prompt, attached, how).spawn();

        let child = match child {
            Ok(child) => child,
            Err(source) => {
                fail(format!("could not start `claude`: {source}"));
                return Err(Error::Io {
                    path: agent.worktree_path.clone(),
                    source,
                });
            }
        };

        let cancel = Arc::new(Notify::new());
        let (settled_tx, settled) = watch::channel(false);
        let task = tokio::spawn(turn::supervise(
            child,
            agent.clone(),
            cancel.clone(),
            settled_tx,
            self.clone(),
        ));

        live.insert(
            agent.id.clone(),
            AgentHandle {
                agent: agent.clone(),
                cancel,
                task,
                settled,
            },
        );
        Ok(agent)
    }

    /// Request Stop for a working Agent. Returns when the supervisor has
    /// finished (meta persisted, state=Stopped). The Worktree is left alone so
    /// the Turn's work survives and the Agent can be Resumed; Discard is a
    /// separate, explicit action.
    ///
    /// The Agent stays in the live map until its supervisor takes it out, so
    /// nothing can start in its Worktree, and a Host waiting to be idle can't
    /// exit, while the Stop is still being recorded.
    pub async fn stop(&self, agent_id: &str) -> Result<()> {
        let (cancel, mut settled) = match self.inner.read().await.get(agent_id) {
            Some(h) => (h.cancel.clone(), h.settled.clone()),
            None => return Err(Error::AgentNotFound(agent_id.to_string())),
        };
        cancel.notify_one();
        // An error means the supervisor is gone, which is settled too.
        let _ = settled.wait_for(|done| *done).await;
        Ok(())
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

    /// Stop taking Turns if none is running, and say whether that happened.
    /// How a Host that is waiting to be updated picks its moment to exit: from
    /// a `true` on, every Spawn and Resume is refused with
    /// [`Error::HostClosing`], so nothing can start in the gap before it does.
    pub async fn close_if_idle(&self) -> bool {
        let live = self.inner.write().await;
        if !live.is_empty() {
            return false;
        }
        self.closed.store(true, Ordering::SeqCst);
        true
    }

    /// Signal every live Agent to stop and wait (bounded) for their
    /// supervisors to finish. Called when the Host stops. Worktrees are *not* discarded
    /// on shutdown — the next Host to start surfaces them as Orphaned via
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
        // Already Orphaned on disk, so the next Host's scan for Agents left
        // Running won't find them; it reads these instead.
        if !handles.is_empty() {
            let ids: Vec<String> = handles.iter().map(|h| h.agent.id.clone()).collect();
            let _ = storage::save_orphan_ids(&ids);
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

/// The record for an Agent the user is Spawning with `prompt`, and the picks
/// and files that go with it. Callers fill in where it branches from.
fn opening(
    project: &Project,
    prompt: String,
    attachments: Vec<PathBuf>,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: Option<String>,
    options: AgentOptions,
) -> Result<Agent> {
    attachments::check(&attachments)?;
    let mut agent = new_agent(project, prompt, model, effort, permission_mode)?;
    agent.task.attachments = attachments;
    agent.options = options;
    Ok(agent)
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
        read_mail: false,
        options: AgentOptions::default(),
        turns: 1,
        // Named at the end of its first Turn, once there is work to name.
        title: None,
        user_title: None,
        color: None,
        spawned_at: now,
        turn_started_at: Some(now),
        exited_at: None,
        exit_code: None,
        fail_reason: None,
        // Nothing has merged yet; a Merge records itself here when it does.
        merged_branch: None,
        merged_at: None,
        unpushed: false,
        push_error: None,
        resolves: None,
        lead_id: None,
        queue: vec![],
    })
}

/// The Agents the last Host left mid-Turn, for a starting Host to report:
/// those it Orphaned as it stopped, and — when it died without stopping —
/// those still marked `Running`, which are Orphaned here with `exited_at = now`.
pub fn adopt_orphans_on_launch() -> Result<Vec<Agent>> {
    let left = storage::take_orphan_ids()?;
    let all = storage::list_agents()?;
    let mut adopted = Vec::new();
    for mut agent in all {
        if agent.state == AgentState::Orphaned && left.contains(&agent.id) {
            adopted.push(agent);
        } else if agent.state == AgentState::Running {
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
