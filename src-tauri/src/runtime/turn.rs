//! One Turn: the `claude` process that runs it, and the supervisor that watches
//! it to the end and records how it went.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use time::OffsetDateTime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Notify;

use super::events::{notice_event, Emitter};
use super::AgentRuntime;
use crate::domain::{Agent, AgentEvent, AgentState, Resolution};
use crate::{attachments, merging, process, storage, title};

/// Grace period between SIGTERM and SIGKILL when stopping an Agent.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// What a Mail-locked Agent's Turns run with on top of `plan` (ADR 0018).
pub(super) const MAIL_LOCKED: [&str; 4] = [
    "--restricted",
    "--strict-mcp-config",
    "--tools",
    "Read,Grep,Glob",
];

/// Whether a Turn opens the Agent's Session or continues it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Continuity {
    Fresh,
    Resumed,
}

/// What every Turn of a Helper is told on top of Claude Code's own system
/// prompt (ADR 0019). Its Task stays what the Lead wrote.
pub(super) fn helper_brief(lead_id: &str) -> String {
    format!(
        "You are a Helper: another Orchestrate Agent, your Lead (id {lead_id}), \
         spawned you to do one part of its task while other Helpers do other parts, \
         each in its own Git worktree. This worktree is on your own branch, cut from \
         your Lead's. Keep to the task you were given. When you finish, commit your \
         work on this branch, since your Lead merges it into its own and uncommitted \
         work is left behind. Don't push, switch branches, or touch any other branch. \
         Your final message is the report your Lead reads: say what you did, what you \
         didn't get to, and anything it needs to know to merge your work."
    )
}

/// The `claude` invocation for one Turn of `agent`, run in its Worktree with
/// `prompt` and any files `attached` to it, and given the app's MCP server
/// if `mcp`.
pub(super) fn command(
    claude_bin: &str,
    mcp: bool,
    agent: &Agent,
    prompt: &str,
    attached: &[PathBuf],
    how: Continuity,
) -> Command {
    let mut cmd = process::command(claude_bin);
    cmd.arg("--print")
        .arg(attachments::for_claude(prompt, attached))
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");
    // Attachments mostly live outside the Worktree, where a plan-mode Turn
    // may not read unless the folder is added. One flag per folder: the
    // option is variadic, and a bare list would swallow what follows it.
    for dir in attachments::dirs(attached) {
        cmd.arg("--add-dir").arg(dir);
    }
    // The app's own MCP server, beside the user's and the Project's rather
    // than instead of them, told which Agent it serves. Its options are
    // variadic too, so a flag follows.
    if mcp {
        cmd.args(crate::mcp::turn_args(&agent.id));
    }
    // The Mode is the Agent's, not this launcher's: `bypassPermissions`
    // lets it work freely inside its Worktree, `plan` holds it to reading
    // and proposing. Unset — a pre-Mode Agent — runs as it always has.
    cmd.arg("--permission-mode").arg(agent.mode());
    // A Mail-locked Agent (ADR 0018) has read what a stranger wrote, so it
    // gets nothing that could act on it or carry what it knows off the
    // machine: no built-in tool but the ones that read files, no MCP server
    // but the app's own and none of its tools that write, and none of the
    // user's settings files, whose allow rules could hand back what this
    // takes away. A `claude` too old for `--restricted` refuses to start,
    // failing the Turn rather than running it loose.
    let mut denied = Vec::new();
    if agent.read_mail {
        cmd.args(MAIL_LOCKED);
        denied.extend(crate::mcp::writing_tools());
    }
    // A Helper can't lead Helpers of its own (ADR 0019), so it isn't offered
    // the tools a Lead uses; the Host would refuse them anyway.
    if let Some(lead) = &agent.lead_id {
        cmd.arg("--append-system-prompt").arg(helper_brief(lead));
        denied.extend(super::mcp::LEAD_TOOLS.map(crate::mcp::turn_name));
    }
    if !denied.is_empty() {
        cmd.arg("--disallowedTools").arg(denied.join(","));
    }
    // No `--model` at all when the user hasn't picked one, so Claude Code's
    // own configured default applies rather than one we guessed.
    if let Some(model) = &agent.model {
        cmd.arg("--model").arg(model);
    }
    // Same for `--effort`: unset means Claude Code's own level.
    if let Some(effort) = &agent.effort {
        cmd.arg("--effort").arg(effort);
    }
    // The Agent's options, layered over the user's own settings. Typed as
    // `/advisor` or `/output-style` they would last this one process.
    if let Some(settings) = agent.options.settings() {
        cmd.arg("--settings").arg(settings);
    }
    if let Some(session) = &agent.session_id {
        match how {
            Continuity::Fresh => cmd.arg("--session-id").arg(session),
            Continuity::Resumed => cmd.arg("--resume").arg(session),
        };
    }
    // A Turn is its process, and `--print` exits once the answer is out:
    // it kills any background shell a few seconds later, still exiting 0.
    // An Agent that backgrounded its build and said "I'll report back"
    // would read as Completed while the work it promised died unseen.
    // With background tasks off, long commands run in the foreground and
    // the Turn really ends when its work does.
    cmd.env("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1");
    cmd.current_dir(&agent.worktree_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

/// The supervisor for one Turn. Runs in a spawned task; owns the Child and
/// reads its stdout to EOF, watching for cancel in the meantime.
pub(super) async fn supervise(
    mut child: Child,
    mut agent: Agent,
    cancel: Arc<Notify>,
    settled: tokio::sync::watch::Sender<bool>,
    runtime: AgentRuntime,
) {
    let emitter = runtime.emitter.clone();
    // The binary to name the Agent with once the Turn ends, or `None` to skip.
    let namer = runtime.naming.then(|| runtime.claude_bin.clone());
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    #[cfg(unix)]
    let pid = child.id();

    // Task: drain stdout as stream-json events.
    let agent_id_out = agent.id.clone();
    let emitter_out = emitter.clone();
    // Whatever session the stream last claimed to be. We told `claude` which ID
    // to use, but trust its own reporting over ours so a version that forks or
    // renames the session stays resumable.
    let observed_session: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
    let observed_out = observed_session.clone();
    // The Turn's final answer, kept for the namer. Overwritten rather than
    // appended: a Turn ends in exactly one result, and that is the one we want.
    let answer: Arc<std::sync::Mutex<String>> = Arc::default();
    let answer_out = answer.clone();
    let read_stdout = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let parsed: Value =
                serde_json::from_str(&line).unwrap_or_else(|_| Value::String(line.clone()));
            if let Some(sid) = parsed.get("session_id").and_then(|v| v.as_str()) {
                *observed_out.lock().unwrap() = Some(sid.to_string());
            }
            if parsed.get("type").and_then(|v| v.as_str()) == Some("result") {
                if let Some(text) = parsed.get("result").and_then(|v| v.as_str()) {
                    *answer_out.lock().unwrap() = text.to_string();
                }
            }
            let event = AgentEvent {
                ts: OffsetDateTime::now_utc(),
                event: parsed,
            };
            emitter_out.record(&agent_id_out, event);
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
            // `shutdown` marks the record Orphaned before it cancels; a user's
            // Stop leaves it Running.
            let orphaned = storage::load_agent(&agent.id)
                .is_ok_and(|on_disk| on_disk.state == AgentState::Orphaned);
            if orphaned {
                Outcome::Orphaned
            } else {
                Outcome::Stopped
            }
        }
        exit = child.wait() => Outcome::Exited(exit),
    };

    // Drain remaining stdout/stderr now that the child is done.
    let _ = read_stdout.await;
    let stderr_buf = read_stderr.await.unwrap_or_default();

    if let Some(sid) = observed_session.lock().unwrap().take() {
        agent.session_id = Some(sid);
    }

    settle(&mut agent, outcome, &stderr_buf);
    let resolved = finish_resolution(&mut agent, &emitter).await;

    // Held across the write, so an edit the user made while the Turn ran (see
    // `AgentRuntime::edit`) can't land between reading it back and saving.
    let mut live = runtime.inner.write().await;
    if let Ok(on_disk) = storage::load_agent(&agent.id) {
        agent.keep_edits(&on_disk);
    }
    let _ = storage::save_agent(&agent);

    // Leave the live map *before* announcing, so a UI that reacts to the exit by
    // sending a follow-up doesn't race the removal and be told we're still busy.
    // A no-op if shutdown() already took us out.
    live.remove(&agent.id);

    // A clean Complete is when the next queued message goes out — still under
    // the lock, so nothing sent meanwhile can jump ahead of it. A Stop or a Fail
    // holds the Queue: interrupting an Agent shouldn't fire the rest in.
    let next = (agent.state == AgentState::Completed && !agent.queue.is_empty())
        .then(|| runtime.next_turn(agent.clone(), &mut live));
    drop(live);

    match next {
        // Announced as it started: the Turn that just Completed is already
        // behind the one that follows it.
        Some(Ok(_)) => {}
        Some(Err(e)) => {
            emitter.record(
                &agent.id,
                notice_event(&format!(
                    "The next queued message wasn't sent: {e}. It is still queued."
                )),
            );
            emitter.announce(&agent);
        }
        None => emitter.announce(&agent),
    }
    settled.send_replace(true);
    if let Some(original) = resolved {
        emitter.announce(&original);
    }

    // Not for an Orphan: its Host is on its way out, and the namer with it.
    let naming = agent.state != AgentState::Orphaned && title::wanted(agent.turns);
    if let Some(namer) = namer.filter(|_| naming) {
        let answer = answer.lock().unwrap().clone();
        // Detached: naming costs a `claude` call of its own, and the Turn's
        // result should reach the UI without waiting on it.
        tokio::spawn(name_agent(namer, agent, Some(answer), emitter));
    }
}

/// Record how the Turn ended on the Agent: its final state, when, and why.
fn settle(agent: &mut Agent, outcome: Outcome, stderr: &str) {
    agent.exited_at = Some(OffsetDateTime::now_utc());
    match outcome {
        Outcome::Stopped => {
            // The Worktree stays: a Stopped Turn may have left work behind, and
            // the Agent can be Resumed to redirect it. Discard is explicit.
            agent.state = AgentState::Stopped;
        }
        Outcome::Orphaned => {
            // Its Host is stopping; the next one to start offers it back.
            agent.state = AgentState::Orphaned;
        }
        Outcome::Exited(Ok(status)) => {
            agent.exit_code = status.code();
            if status.success() {
                agent.state = AgentState::Completed;
            } else {
                agent.state = AgentState::Failed;
                agent.fail_reason = if stderr.trim().is_empty() {
                    Some(format!("exit code {:?}", status.code()))
                } else {
                    Some(stderr.trim().to_string())
                };
            }
        }
        Outcome::Exited(Err(e)) => {
            agent.state = AgentState::Failed;
            agent.fail_reason = Some(format!("wait error: {e}"));
        }
    }
}

/// A Resolver whose Turn ended cleanly has, if it did its job, a branch that
/// merges without conflict: finish the Merge the user asked for when they
/// spawned it, and say in its transcript how that went. Only once — a Resolver
/// Resumed after that is an ordinary Agent whose work the user Merges by hand.
///
/// Returns the Agent it resolved, if the Merge recorded that one too.
async fn finish_resolution(agent: &mut Agent, emitter: &Emitter) -> Option<Agent> {
    let Resolution { target, .. } = agent.resolves.clone()?;
    if agent.state != AgentState::Completed || agent.merged_at.is_some() {
        return None;
    }
    let mut resolved = None;
    let text = match merging::finish_resolution(agent, &target).await {
        Ok((merged, original)) => {
            resolved = original;
            format!(
                "Conflicts resolved: merged into `{target}` as {}.",
                &merged.sha[..merged.sha.len().min(7)]
            )
        }
        Err(e) => format!(
            "Not merged: {e}. Reply to finish the job, or merge from the diff once it's ready."
        ),
    };
    emitter.record(&agent.id, notice_event(&text));
    resolved
}

/// Ask Claude for a short name for this Agent and record it. Re-reads the meta
/// file rather than writing back the snapshot we were handed: by the time a
/// name comes back, the user may already have Resumed the Agent, and only the
/// title is ours to change.
///
/// `answer` is `None` for the name an Agent gets as it spawns, drawn from its
/// Task alone. That one only fills a blank: if the first Turn was quick enough
/// to be named from its answer first, the better name stays.
pub(super) async fn name_agent(
    namer: Arc<String>,
    agent: Agent,
    answer: Option<String>,
    emitter: Emitter,
) {
    // The Worktree is the namer's cwd; a Discarded Agent has nowhere to run.
    if !agent.worktree_path.exists() {
        return;
    }
    let Some(title) = title::generate(
        &namer,
        &agent.worktree_path,
        &agent.task.prompt,
        answer.as_deref().unwrap_or_default(),
        agent.read_mail,
    )
    .await
    else {
        return;
    };

    let Ok(mut fresh) = storage::load_agent(&agent.id) else {
        return; // Discarded while we were naming it.
    };
    if answer.is_none() && fresh.title.is_some() {
        return;
    }
    // Last name back wins, even if a later Turn has since started or finished.
    // Two namers can land out of order, but the worst case is a name drawn from
    // Turn 2 rather than Turn 3 — better than the alternative, where an Agent
    // whose Turns are queued back-to-back outruns every namer and stays unnamed.
    fresh.title = Some(title);
    if storage::save_agent(&fresh).is_err() {
        return;
    }
    emitter.announce(&fresh);
}

enum Outcome {
    Stopped,
    Orphaned,
    Exited(std::io::Result<std::process::ExitStatus>),
}

#[cfg(unix)]
fn signal_term(pid: u32) {
    // SAFETY: libc::kill is safe to call from Rust; pid is a u32 that fits pid_t.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}
