use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::task::JoinHandle;

use crate::error::{Error, Result};
use crate::model::{new_id, new_session_id, Agent, AgentEvent, AgentState, Id, Project, Task};
use crate::{git, paths, storage, worktree};

/// Grace period between SIGTERM and SIGKILL when stopping an Agent.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How long shutdown() waits for supervisor tasks to finish per Agent.
const SHUTDOWN_TIMEOUT_PER_AGENT: Duration = Duration::from_secs(5);

/// Event type for a prompt the *user* sent. Our own invention — `claude` never
/// emits it — so the transcript can show both sides of the conversation.
pub const PROMPT_EVENT_TYPE: &str = "cw_prompt";

/// Record a user prompt as a log event, so a Turn's question sits above its
/// answer in the output pane and survives a reload.
fn prompt_event(prompt: &str, turn: u32) -> AgentEvent {
    AgentEvent {
        ts: OffsetDateTime::now_utc(),
        event: serde_json::json!({
            "type": PROMPT_EVENT_TYPE,
            "prompt": prompt,
            "turn": turn,
        }),
    }
}

/// Events emitted by the runtime for consumers (Tauri IPC in Phase 3).
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// One line from a running Agent's stdout, parsed and timestamped.
    AgentEvent { agent_id: Id, event: AgentEvent },
    /// The Agent's persisted state changed (state, exit_code, exited_at).
    StateChanged { agent_id: Id, agent: Agent },
}

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
    inner: Arc<RwLock<HashMap<Id, AgentHandle>>>,
    events_tx: mpsc::UnboundedSender<RuntimeEvent>,
    /// The `claude` binary to invoke. Overridable in tests.
    claude_bin: Arc<String>,
}

impl AgentRuntime {
    /// Construct a runtime and the receiver half of its event stream.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            events_tx: tx,
            claude_bin: Arc::new("claude".to_string()),
        };
        (rt, rx)
    }

    /// Construct with a custom binary path (for testing with a fake claude).
    #[cfg(test)]
    pub fn with_bin(bin: impl Into<String>) -> (Self, mpsc::UnboundedReceiver<RuntimeEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            events_tx: tx,
            claude_bin: Arc::new(bin.into()),
        };
        (rt, rx)
    }

    /// Spawn a new Agent for the given Project and opening prompt, on the model
    /// the user picked (`None` leaves the choice to Claude Code). Returns the
    /// Agent record once the process is running and its meta file is on disk.
    pub async fn spawn(
        &self,
        project: &Project,
        prompt: String,
        model: Option<String>,
    ) -> Result<Agent> {
        let agent_id = new_id();
        let branch = format!("cw/agent-{agent_id}");
        let worktree_path = paths::worktrees_dir()?
            .join(&project.id)
            .join(&agent_id);

        // Record the commit we branched from before the Agent can move HEAD, so
        // the diff view has a fixed base even if the Project advances later.
        let base_commit = git::head_commit(&project.path).await.ok();

        worktree::create(&project.path, &worktree_path, &branch).await?;

        let agent = Agent {
            id: agent_id.clone(),
            project_id: project.id.clone(),
            task: Task { prompt: prompt.clone() },
            state: AgentState::Running,
            worktree_path: worktree_path.clone(),
            branch: branch.clone(),
            base_commit,
            // Mint the Session ID rather than waiting to read it off the
            // stream, so the Agent is resumable even if it dies mid-first-line.
            session_id: Some(new_session_id()),
            model,
            turns: 1,
            spawned_at: OffsetDateTime::now_utc(),
            exited_at: None,
            exit_code: None,
            fail_reason: None,
        };
        storage::save_agent(&agent)?;
        self.record_prompt(&agent, &prompt);

        let mut live = self.inner.write().await;
        self.launch(agent, &prompt, Continuity::Fresh, &mut live)
    }

    /// Continue an Agent's conversation with a follow-up prompt: start a new
    /// `claude` in the Agent's existing Worktree, resuming its Session, and put
    /// the Agent back into `Running`.
    ///
    /// `model` is the choice for this Turn onwards, and is recorded on the
    /// Agent — a user may start cheap and escalate mid-conversation. The caller
    /// always states it, so `None` means "let Claude Code pick", not "unchanged".
    ///
    /// Refused while the Agent is working, and for an Agent with no Session or
    /// no Worktree left to work in.
    pub async fn resume(
        &self,
        agent_id: &str,
        prompt: String,
        model: Option<String>,
    ) -> Result<Agent> {
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
        agent.state = AgentState::Running;
        agent.exited_at = None;
        agent.exit_code = None;
        agent.fail_reason = None;
        storage::save_agent(&agent)?;
        self.record_prompt(&agent, &prompt);
        self.announce(&agent);

        self.launch(agent, &prompt, Continuity::Resumed, &mut live)
    }

    /// Append the user's prompt to the Agent's log and push it to the UI.
    fn record_prompt(&self, agent: &Agent, prompt: &str) {
        let event = prompt_event(prompt, agent.turns);
        let _ = storage::append_event(&agent.id, &event);
        let _ = self.events_tx.send(RuntimeEvent::AgentEvent {
            agent_id: agent.id.clone(),
            event,
        });
    }

    /// Tell consumers the Agent's persisted record moved.
    fn announce(&self, agent: &Agent) {
        let _ = self.events_tx.send(RuntimeEvent::StateChanged {
            agent_id: agent.id.clone(),
            agent: agent.clone(),
        });
    }

    /// Start one Turn: spin up `claude` in the Agent's Worktree and hand the
    /// child to a supervisor. A Turn that cannot even be started is a Fail, so
    /// the Agent never sits in `Running` with no process behind it.
    fn launch(
        &self,
        agent: Agent,
        prompt: &str,
        how: Continuity,
        live: &mut Live<'_>,
    ) -> Result<Agent> {
        let mut cmd = Command::new(self.claude_bin.as_str());
        cmd.arg("--print")
            .arg(prompt)
            .arg("--output-format").arg("stream-json")
            .arg("--verbose")
            .arg("--permission-mode").arg("bypassPermissions");
        // No `--model` at all when the user hasn't picked one, so Claude Code's
        // own configured default applies rather than one we guessed.
        if let Some(model) = &agent.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(session) = &agent.session_id {
            match how {
                Continuity::Fresh => cmd.arg("--session-id").arg(session),
                Continuity::Resumed => cmd.arg("--resume").arg(session),
            };
        }
        let child = cmd
            .current_dir(&agent.worktree_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let child = match child {
            Ok(child) => child,
            Err(source) => {
                let mut failed = agent.clone();
                failed.state = AgentState::Failed;
                failed.exited_at = Some(OffsetDateTime::now_utc());
                failed.fail_reason = Some(format!("could not start `claude`: {source}"));
                let _ = storage::save_agent(&failed);
                self.announce(&failed);
                return Err(Error::Io {
                    path: agent.worktree_path.clone(),
                    source,
                });
            }
        };

        let cancel = Arc::new(Notify::new());
        let cancel_task = cancel.clone();
        let events_tx = self.events_tx.clone();
        let inner = self.inner.clone();
        let agent_for_task = agent.clone();

        let task = tokio::spawn(async move {
            supervise(child, agent_for_task, cancel_task, events_tx, inner).await;
        });

        live.insert(
            agent.id.clone(),
            AgentHandle { agent: agent.clone(), cancel, task },
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

/// The supervisor for one Turn. Runs in a spawned task; owns the Child and
/// reads its stdout to EOF, watching for cancel in the meantime.
async fn supervise(
    mut child: Child,
    mut agent: Agent,
    cancel: Arc<Notify>,
    events_tx: mpsc::UnboundedSender<RuntimeEvent>,
    inner: Arc<RwLock<HashMap<Id, AgentHandle>>>,
) {
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let pid = child.id();

    // Task: drain stdout as stream-json events.
    let agent_id_out = agent.id.clone();
    let events_tx_out = events_tx.clone();
    // Whatever session the stream last claimed to be. We told `claude` which ID
    // to use, but trust its own reporting over ours so a version that forks or
    // renames the session stays resumable.
    let observed_session: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
    let observed_out = observed_session.clone();
    let read_stdout = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let parsed: Value = serde_json::from_str(&line)
                .unwrap_or_else(|_| Value::String(line.clone()));
            if let Some(sid) = parsed.get("session_id").and_then(|v| v.as_str()) {
                *observed_out.lock().unwrap() = Some(sid.to_string());
            }
            let event = AgentEvent {
                ts: OffsetDateTime::now_utc(),
                event: parsed,
            };
            let _ = storage::append_event(&agent_id_out, &event);
            let _ = events_tx_out.send(RuntimeEvent::AgentEvent {
                agent_id: agent_id_out.clone(),
                event,
            });
        }
    });

    // Task: drain stderr into a buffer so we can put it in fail_reason.
    let read_stderr = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&line);
        }
        buf
    });

    // Race the child's exit against a Stop signal.
    let outcome = tokio::select! {
        biased;
        _ = cancel.notified() => {
            #[cfg(unix)]
            if let Some(pid) = pid {
                signal_term(pid);
            }
            let waited = tokio::time::timeout(STOP_GRACE, child.wait()).await;
            if waited.is_err() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            Outcome::Stopped
        }
        exit = child.wait() => Outcome::Exited(exit),
    };

    // Drain remaining stdout/stderr now that the child is done.
    let _ = read_stdout.await;
    let stderr_buf = read_stderr.await.unwrap_or_default();

    if let Some(sid) = observed_session.lock().unwrap().take() {
        agent.session_id = Some(sid);
    }

    // Compute final Agent state.
    let now = OffsetDateTime::now_utc();
    agent.exited_at = Some(now);
    match outcome {
        Outcome::Stopped => {
            // The Worktree stays: a Stopped Turn may have left work behind, and
            // the Agent can be Resumed to redirect it. Reap is explicit.
            agent.state = AgentState::Stopped;
        }
        Outcome::Exited(Ok(status)) => {
            agent.exit_code = status.code();
            if status.success() {
                agent.state = AgentState::Completed;
            } else {
                agent.state = AgentState::Failed;
                agent.fail_reason = if stderr_buf.trim().is_empty() {
                    Some(format!("exit code {:?}", status.code()))
                } else {
                    Some(stderr_buf.trim().to_string())
                };
            }
        }
        Outcome::Exited(Err(e)) => {
            agent.state = AgentState::Failed;
            agent.fail_reason = Some(format!("wait error: {e}"));
        }
    }

    let _ = storage::save_agent(&agent);

    // Leave the live map *before* announcing, so a UI that reacts to the exit by
    // sending a follow-up doesn't race the removal and be told we're still busy.
    // A no-op if stop() already took us out.
    inner.write().await.remove(&agent.id);

    let _ = events_tx.send(RuntimeEvent::StateChanged {
        agent_id: agent.id.clone(),
        agent: agent.clone(),
    });
}

enum Outcome {
    Stopped,
    Exited(std::io::Result<std::process::ExitStatus>),
}

/// Whether a Turn opens the Agent's Session or continues it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Continuity {
    Fresh,
    Resumed,
}

#[cfg(unix)]
fn signal_term(pid: u32) {
    // SAFETY: libc::kill is safe to call from Rust; pid is a u32 that fits pid_t.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::StateEnv;
    use tempfile::TempDir;

    async fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        for args in [
            vec!["init", "--initial-branch=main"],
            vec!["config", "user.email", "t@t.t"],
            vec!["config", "user.name", "t"],
            // Don't let the developer's global config break the fixture.
            vec!["config", "commit.gpgsign", "false"],
            vec!["config", "core.hooksPath", "/dev/null"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let out = Command::new("git")
                .arg("-C").arg(dir.path())
                .args(&args)
                .output()
                .await
                .unwrap();
            assert!(out.status.success());
        }
        dir
    }

    fn sample_project(path: std::path::PathBuf) -> Project {
        Project {
            id: new_id(),
            name: "T".into(),
            path,
            added_at: OffsetDateTime::now_utc(),
        }
    }

    /// A fake `claude` that emits three stream-json lines then exits 0.
    fn fake_claude_ok() -> String {
        let script = r#"#!/bin/sh
echo '{"type":"system","event":"init"}'
echo '{"type":"assistant","message":"hi"}'
echo '{"type":"result","status":"complete"}'
exit 0
"#;
        write_script(script)
    }

    fn fake_claude_fail() -> String {
        let script = r#"#!/bin/sh
echo '{"type":"system","event":"init"}' >&2
echo 'boom' >&2
exit 42
"#;
        write_script(script)
    }

    fn fake_claude_hang() -> String {
        let script = r#"#!/bin/sh
echo '{"type":"system","event":"init"}'
# Ignore SIGTERM so we can test SIGKILL fallback path.
trap 'echo "ignored TERM"' TERM
while : ; do sleep 1; done
"#;
        write_script(script)
    }

    fn write_script(contents: &str) -> String {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("cw-fake-claude-{}.sh", new_id()));
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        let mut perms = std::fs::metadata(&tmp).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp, perms).unwrap();
        tmp.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn spawn_run_to_completion() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = fake_claude_ok();
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "hello".into(), None).await.unwrap();
        assert_eq!(agent.state, AgentState::Running);

        // Consume events until we see StateChanged (final).
        let final_agent = loop {
            match rx.recv().await.expect("channel open") {
                RuntimeEvent::AgentEvent { .. } => continue,
                RuntimeEvent::StateChanged { agent, .. } => break agent,
            }
        };
        assert_eq!(final_agent.state, AgentState::Completed);
        assert_eq!(final_agent.exit_code, Some(0));
        assert!(rt.running().await.is_empty());

        // Log file has the user's prompt plus the three streamed events.
        let log = std::fs::read_to_string(paths::agent_log_path(&agent.id).unwrap()).unwrap();
        assert_eq!(log.lines().count(), 4);
        let first: AgentEvent = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(first.event["type"], PROMPT_EVENT_TYPE);
        assert_eq!(first.event["prompt"], "hello");
    }

    /// A fake `claude` that behaves like a real one that got partway: it commits
    /// some work on the Agent's branch and leaves the rest dirty.
    fn fake_claude_writes_code() -> String {
        write_script(
            r#"#!/bin/sh
echo '{"type":"system","event":"init"}'
echo 'from the agent' > added.txt
git add -A
git commit -qm 'agent: add a file'
echo 'left dirty' > dirty.txt
exit 0
"#,
        )
    }

    #[tokio::test]
    async fn spawned_agents_worktree_is_diffable_after_completion() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_writes_code());

        let agent = rt.spawn(&project, "write some code".into(), None).await.unwrap();
        let base = agent
            .base_commit
            .clone()
            .expect("spawn records the commit it branched from");

        let final_agent = loop {
            match rx.recv().await.expect("channel open") {
                RuntimeEvent::AgentEvent { .. } => continue,
                RuntimeEvent::StateChanged { agent, .. } => break agent,
            }
        };
        assert_eq!(final_agent.state, AgentState::Completed);
        assert_eq!(final_agent.base_commit.as_deref(), Some(base.as_str()));

        // The diff the UI would show: committed and dirty work side by side.
        let diff = crate::git::diff(
            Some(&project.path),
            &final_agent.worktree_path,
            &final_agent.branch,
            final_agent.base_commit.as_deref(),
        )
        .await
        .unwrap();

        let paths: Vec<&str> = diff.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"added.txt"), "{paths:?}");
        assert!(paths.contains(&"dirty.txt"), "{paths:?}");
        assert_eq!(diff.commits.len(), 1, "agent's own commit should show");
        assert!(diff.uncommitted, "dirty.txt is not committed yet");

        // And committing from the app captures what the Agent left behind.
        crate::git::commit(&final_agent.worktree_path, "save the rest")
            .await
            .unwrap();
        let after = crate::git::diff(
            Some(&project.path),
            &final_agent.worktree_path,
            &final_agent.branch,
            final_agent.base_commit.as_deref(),
        )
        .await
        .unwrap();
        assert!(!after.uncommitted);
        assert_eq!(after.commits.len(), 2);
    }

    #[tokio::test]
    async fn spawn_run_to_failure() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = fake_claude_fail();
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "boom".into(), None).await.unwrap();
        let final_agent = loop {
            match rx.recv().await.unwrap() {
                RuntimeEvent::StateChanged { agent, .. } => break agent,
                _ => {}
            }
        };
        assert_eq!(final_agent.state, AgentState::Failed);
        assert_eq!(final_agent.exit_code, Some(42));
        assert!(
            final_agent.fail_reason.as_deref().unwrap_or("").contains("boom"),
            "fail_reason should carry stderr; got {:?}",
            final_agent.fail_reason
        );
        // Worktree preserved on failure.
        assert!(agent.worktree_path.exists());
    }

    #[tokio::test]
    async fn stop_preserves_the_worktree_for_review() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = fake_claude_hang();
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "hang".into(), None).await.unwrap();
        assert!(agent.worktree_path.exists());

        // Give the hang script a moment to install its TERM trap.
        tokio::time::sleep(Duration::from_millis(200)).await;
        rt.stop(&agent.id).await.unwrap();

        // Drain the final StateChanged.
        while let Some(ev) = rx.recv().await {
            if let RuntimeEvent::StateChanged { agent, .. } = ev {
                assert_eq!(agent.state, AgentState::Stopped);
                break;
            }
        }
        assert!(
            agent.worktree_path.exists(),
            "Stop leaves the worktree; Reap is the explicit action that removes it"
        );
    }

    /// Waits for the Turn to end — skipping the `Running` announcement a Resume
    /// makes on its way in.
    async fn wait_for_exit(rx: &mut mpsc::UnboundedReceiver<RuntimeEvent>) -> Agent {
        loop {
            match rx.recv().await.expect("channel open") {
                RuntimeEvent::StateChanged { agent, .. }
                    if agent.state != AgentState::Running =>
                {
                    break agent
                }
                _ => continue,
            }
        }
    }

    /// A fake `claude` that appends its argv to `args_log` before replying, so a
    /// test can check which session flags we passed.
    fn fake_claude_recording(args_log: &std::path::Path) -> String {
        write_script(&format!(
            r#"#!/bin/sh
echo "$@" >> {log}
echo '{{"type":"assistant","message":"hi"}}'
echo '{{"type":"result","status":"complete"}}'
exit 0
"#,
            log = args_log.display()
        ))
    }

    #[tokio::test]
    async fn resume_continues_the_same_session_and_worktree() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

        let agent = rt.spawn(&project, "first".into(), None).await.unwrap();
        let session = agent.session_id.clone().expect("spawn mints a session");
        let after_first = wait_for_exit(&mut rx).await;
        assert_eq!(after_first.state, AgentState::Completed);
        assert_eq!(after_first.turns, 1);

        let resumed = rt.resume(&agent.id, "second".into(), None).await.unwrap();
        assert_eq!(resumed.state, AgentState::Running);
        assert_eq!(resumed.turns, 2, "a follow-up is a new Turn on the same Agent");
        assert_eq!(resumed.worktree_path, agent.worktree_path, "same sandbox");
        assert_eq!(resumed.session_id.as_deref(), Some(session.as_str()));

        let after_second = wait_for_exit(&mut rx).await;
        assert_eq!(after_second.state, AgentState::Completed);
        assert_eq!(after_second.turns, 2);

        // The opening Turn started the session; the follow-up resumed it.
        let args = std::fs::read_to_string(&args_log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(lines.len(), 2, "one `claude` per Turn: {args}");
        assert!(lines[0].contains(&format!("--session-id {session}")), "{}", lines[0]);
        assert!(lines[1].contains(&format!("--resume {session}")), "{}", lines[1]);
        assert!(!lines[1].contains("--session-id"), "{}", lines[1]);

        // Both prompts are in the transcript, each tagged with its Turn.
        let logged = storage::read_events(&agent.id).unwrap();
        let prompts: Vec<(&str, u64)> = logged
            .iter()
            .filter(|e| e.event["type"] == PROMPT_EVENT_TYPE)
            .map(|e| {
                (
                    e.event["prompt"].as_str().unwrap(),
                    e.event["turn"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(prompts, vec![("first", 1), ("second", 2)]);
    }

    #[tokio::test]
    async fn the_picked_model_is_passed_to_claude_and_carried_across_turns() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

        let agent = rt
            .spawn(&project, "first".into(), Some("sonnet".into()))
            .await
            .unwrap();
        assert_eq!(agent.model.as_deref(), Some("sonnet"));
        wait_for_exit(&mut rx).await;

        // A user who starts cheap can escalate without starting over.
        let resumed = rt
            .resume(&agent.id, "try harder".into(), Some("opus".into()))
            .await
            .unwrap();
        assert_eq!(resumed.model.as_deref(), Some("opus"));
        wait_for_exit(&mut rx).await;
        assert_eq!(
            storage::load_agent(&agent.id).unwrap().model.as_deref(),
            Some("opus"),
            "the new choice must be persisted, or the next Turn reverts"
        );

        let args = std::fs::read_to_string(&args_log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(lines.len(), 2, "one `claude` per Turn: {args}");
        assert!(lines[0].contains("--model sonnet"), "{}", lines[0]);
        assert!(lines[1].contains("--model opus"), "{}", lines[1]);
    }

    /// No pick means no flag at all, so Claude Code's own default applies
    /// rather than one we guessed on the user's behalf.
    #[tokio::test]
    async fn no_picked_model_passes_no_model_flag() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

        let agent = rt.spawn(&project, "hello".into(), None).await.unwrap();
        assert_eq!(agent.model, None);
        wait_for_exit(&mut rx).await;

        let args = std::fs::read_to_string(&args_log).unwrap();
        assert!(!args.contains("--model"), "{args}");
    }

    #[tokio::test]
    async fn resume_is_refused_while_the_agent_is_working() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let (rt, _rx) = AgentRuntime::with_bin(fake_claude_hang());

        let agent = rt.spawn(&project, "hang".into(), None).await.unwrap();
        let err = rt.resume(&agent.id, "hurry up".into(), None).await.unwrap_err();
        assert!(matches!(err, Error::AgentBusy(_)), "{err:?}");

        rt.stop(&agent.id).await.unwrap();
    }

    #[tokio::test]
    async fn a_stopped_agent_can_still_be_resumed() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_hang());

        let agent = rt.spawn(&project, "go off the rails".into(), None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        rt.stop(&agent.id).await.unwrap();
        let stopped = wait_for_exit(&mut rx).await;
        assert_eq!(stopped.state, AgentState::Stopped);

        // Redirecting a Stopped Agent is the point of keeping its worktree.
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));
        let resumed = rt.resume(&agent.id, "do it differently".into(), None).await.unwrap();
        assert_eq!(resumed.turns, 2);
        assert_eq!(wait_for_exit(&mut rx).await.state, AgentState::Completed);
    }

    #[tokio::test]
    async fn resume_is_refused_when_the_worktree_is_gone() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_ok());

        let agent = rt.spawn(&project, "hello".into(), None).await.unwrap();
        wait_for_exit(&mut rx).await;
        worktree::reap(&project.path, &agent.worktree_path, &agent.branch)
            .await
            .unwrap();

        let err = rt.resume(&agent.id, "more".into(), None).await.unwrap_err();
        assert!(
            matches!(&err, Error::NotResumable { why, .. } if why.contains("worktree")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn resume_is_refused_for_an_agent_with_no_session() {
        let _env = StateEnv::new();
        let a = Agent {
            id: new_id(),
            project_id: new_id(),
            task: Task { prompt: "from before sessions existed".into() },
            state: AgentState::Completed,
            worktree_path: std::env::temp_dir(),
            base_commit: None,
            session_id: None,
            model: None,
            turns: 1,
            branch: "cw/agent-old".into(),
            spawned_at: OffsetDateTime::now_utc(),
            exited_at: Some(OffsetDateTime::now_utc()),
            exit_code: Some(0),
            fail_reason: None,
        };
        storage::save_agent(&a).unwrap();
        let (rt, _rx) = AgentRuntime::with_bin(fake_claude_ok());

        let err = rt.resume(&a.id, "carry on".into(), None).await.unwrap_err();
        assert!(
            matches!(&err, Error::NotResumable { why, .. } if why.contains("session")),
            "{err:?}"
        );
    }

    /// If `claude` reports a session other than the one we asked for, believe it
    /// — that ID is what a later Resume has to pass.
    #[tokio::test]
    async fn a_session_id_from_the_stream_wins_over_ours() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = write_script(
            r#"#!/bin/sh
echo '{"type":"system","subtype":"init","session_id":"deadbeef-0000-4000-8000-000000000001"}'
exit 0
"#,
        );
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "hello".into(), None).await.unwrap();
        let minted = agent.session_id.clone().unwrap();
        let done = wait_for_exit(&mut rx).await;
        assert_ne!(done.session_id.as_deref(), Some(minted.as_str()));
        assert_eq!(
            done.session_id.as_deref(),
            Some("deadbeef-0000-4000-8000-000000000001")
        );
        assert_eq!(
            storage::load_agent(&agent.id).unwrap().session_id,
            done.session_id,
            "the corrected session must be persisted, or Resume passes the wrong one"
        );
    }

    #[tokio::test]
    async fn adopt_orphans_transitions_running() {
        let _env = StateEnv::new();
        // Seed a Running agent on disk (as if it were running when the app died).
        let a = Agent {
            id: new_id(),
            project_id: new_id(),
            task: Task { prompt: "x".into() },
            state: AgentState::Running,
            worktree_path: "/tmp/wt".into(),
            base_commit: None,
            session_id: Some("11111111-2222-4333-8444-555555555555".into()),
            model: None,
            turns: 1,
            branch: "cw/agent-x".into(),
            spawned_at: OffsetDateTime::now_utc(),
            exited_at: None,
            exit_code: None,
            fail_reason: None,
        };
        storage::save_agent(&a).unwrap();

        let orphans = adopt_orphans_on_launch().unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].state, AgentState::Orphaned);
        assert!(orphans[0].exited_at.is_some());

        // Idempotent: running it again finds nothing.
        let again = adopt_orphans_on_launch().unwrap();
        assert!(again.is_empty());
    }
}
