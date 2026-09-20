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
use crate::model::{new_id, Agent, AgentEvent, AgentState, Id, Project, Task};
use crate::{paths, storage, worktree};

/// Grace period between SIGTERM and SIGKILL when stopping an Agent.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How long shutdown() waits for supervisor tasks to finish per Agent.
const SHUTDOWN_TIMEOUT_PER_AGENT: Duration = Duration::from_secs(5);

/// Events emitted by the runtime for consumers (Tauri IPC in Phase 3).
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// One line from a running Agent's stdout, parsed and timestamped.
    AgentEvent { agent_id: Id, event: AgentEvent },
    /// The Agent's persisted state changed (state, exit_code, exited_at).
    StateChanged { agent_id: Id, agent: Agent },
}

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

    /// Spawn a new Agent for the given Project and prompt. Returns the Agent
    /// record once the process is running and its meta file is on disk.
    pub async fn spawn(&self, project: &Project, prompt: String) -> Result<Agent> {
        let agent_id = new_id();
        let branch = format!("cw/agent-{agent_id}");
        let worktree_path = paths::worktrees_dir()?
            .join(&project.id)
            .join(&agent_id);

        worktree::create(&project.path, &worktree_path, &branch).await?;

        let agent = Agent {
            id: agent_id.clone(),
            project_id: project.id.clone(),
            task: Task { prompt: prompt.clone() },
            state: AgentState::Running,
            worktree_path: worktree_path.clone(),
            branch: branch.clone(),
            spawned_at: OffsetDateTime::now_utc(),
            exited_at: None,
            exit_code: None,
            fail_reason: None,
        };
        storage::save_agent(&agent)?;

        let child = Command::new(self.claude_bin.as_str())
            .arg("--print")
            .arg(&prompt)
            .arg("--output-format").arg("stream-json")
            .arg("--verbose")
            .arg("--permission-mode").arg("bypassPermissions")
            .current_dir(&worktree_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false)
            .spawn()
            .map_err(|source| Error::Io {
                path: worktree_path.clone(),
                source,
            })?;

        let cancel = Arc::new(Notify::new());
        let cancel_task = cancel.clone();
        let events_tx = self.events_tx.clone();
        let inner = self.inner.clone();
        let project_path = project.path.clone();
        let agent_for_task = agent.clone();

        let task = tokio::spawn(async move {
            supervise(child, agent_for_task, project_path, cancel_task, events_tx, inner).await;
        });

        self.inner.write().await.insert(
            agent_id,
            AgentHandle { agent: agent.clone(), cancel, task },
        );
        Ok(agent)
    }

    /// Request Stop for a running Agent. Returns when the supervisor has
    /// finished (worktree reaped, meta persisted, state=Stopped).
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

/// The per-Agent supervisor. Runs in a spawned task; owns the Child and
/// reads its stdout to EOF, watching for cancel in the meantime.
async fn supervise(
    mut child: Child,
    mut agent: Agent,
    project_path: std::path::PathBuf,
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
    let read_stdout = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let parsed: Value = serde_json::from_str(&line)
                .unwrap_or_else(|_| Value::String(line.clone()));
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

    // Compute final Agent state.
    let now = OffsetDateTime::now_utc();
    agent.exited_at = Some(now);
    match outcome {
        Outcome::Stopped => {
            agent.state = AgentState::Stopped;
            // On Stop, reap the worktree per V1 design.
            let _ = worktree::reap(&project_path, &agent.worktree_path, &agent.branch).await;
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
    let _ = events_tx.send(RuntimeEvent::StateChanged {
        agent_id: agent.id.clone(),
        agent: agent.clone(),
    });

    // Remove from live map if we haven't been removed by stop() already.
    let mut inner = inner.write().await;
    inner.remove(&agent.id);
}

enum Outcome {
    Stopped,
    Exited(std::io::Result<std::process::ExitStatus>),
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

        let agent = rt.spawn(&project, "hello".into()).await.unwrap();
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

        // Log file has 3 lines.
        let log = std::fs::read_to_string(paths::agent_log_path(&agent.id).unwrap()).unwrap();
        assert_eq!(log.lines().count(), 3);
    }

    #[tokio::test]
    async fn spawn_run_to_failure() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = fake_claude_fail();
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "boom".into()).await.unwrap();
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
    async fn stop_reaps_worktree() {
        let _env = StateEnv::new();
        let repo = init_repo().await;
        let project = sample_project(repo.path().to_path_buf());
        let bin = fake_claude_hang();
        let (rt, mut rx) = AgentRuntime::with_bin(bin);

        let agent = rt.spawn(&project, "hang".into()).await.unwrap();
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
        assert!(!agent.worktree_path.exists(), "worktree should be reaped on Stop");
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
