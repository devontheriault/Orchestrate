//! Everything a window can ask of its Host. Each [`Call`] is what used to be a
//! Tauri command, and answers the same way: JSON on success, a readable message
//! on failure, which the webview sees as a plain `Error`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use super::Host;
use crate::domain::{new_id, Agent, AgentOptions, AgentState, Project, QueuedMessage};
use crate::error::Error;
use crate::git::{self, Merged};
use crate::{merging, paths, storage, worktree};

/// One call, as `{"method": "spawn_agent", "args": {"projectId": ...}}`. The
/// argument names are the webview's, camelCase, as Tauri commands took them.
/// Every variant has braces, even with nothing in them, so `args: {}` reads the
/// same for all of them.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Call {
    ListProjects {},
    /// Whether the folder needs [`git::set_up`] before Agents can work in it.
    /// Asked when the user picks a folder, so the trust dialog can say so.
    ProjectNeedsSetup {
        path: PathBuf,
    },
    /// Register a folder as a Project. `set_up` is the user's go-ahead to
    /// [`git::set_up`] one that isn't ready; without it, such a folder is
    /// refused here rather than failing later, at the first Spawn.
    AddProject {
        name: String,
        path: PathBuf,
        set_up: bool,
    },
    RemoveProject {
        id: String,
    },
    ListAgents {},
    SpawnAgent {
        project_id: String,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        options: AgentOptions,
    },
    /// Say something to an Agent, on the model, effort and Permission Mode the
    /// caller picked for it. The Host decides whether it starts a Turn now or
    /// waits in the Agent's Queue; the window never has to.
    SendMessage {
        agent_id: String,
        prompt: String,
        attachments: Vec<PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
    },
    /// Send the head of a Queue that a Stop or a Fail held.
    SendNext {
        agent_id: String,
    },
    /// Add messages to the end of an Agent's Queue without sending any. How a
    /// window hands over the Queues it kept itself before the Host kept them.
    QueueMessages {
        agent_id: String,
        messages: Vec<QueuedMessage>,
    },
    RemoveQueued {
        agent_id: String,
        message_id: String,
    },
    ClearQueue {
        agent_id: String,
    },
    /// Keep an attachment's bytes on this Host, and say where. The path is what
    /// a Spawn or a message then attaches: the Agent reads the Host's copy.
    StoreAttachment {
        name: String,
        /// The file's bytes, base64.
        data: String,
    },
    /// Whether this Host keeps running while its user is logged out: `null`
    /// when it isn't a service that could.
    KeepRunning {},
    SetKeepRunning {
        on: bool,
    },
    /// Give an Agent the user's own Title, or with `None` hand the naming back
    /// to Claude. Allowed while it works, like the other edits.
    RenameAgent {
        agent_id: String,
        name: Option<String>,
    },
    SetAgentColor {
        agent_id: String,
        color: Option<String>,
    },
    /// Set how an Agent's Turns run from its next one on.
    SetAgentOptions {
        agent_id: String,
        options: AgentOptions,
    },
    StopAgent {
        agent_id: String,
    },
    DiscardAgent {
        agent_id: String,
    },
    /// Replay of the Agent's output, read back from its log on disk. Lets a
    /// window rebuild the output pane after a reload or a reconnection without
    /// disturbing a running Agent.
    AgentEvents {
        agent_id: String,
    },
    /// Everything the Agent has produced in its Worktree, relative to its Base.
    /// Safe in any state, including while the Agent runs.
    AgentDiff {
        agent_id: String,
    },
    /// Stage and commit everything in the Agent's Worktree, on its own branch.
    AgentCommit {
        agent_id: String,
        message: String,
    },
    /// The Project's local branches, for the Merge picker.
    ProjectBranches {
        project_id: String,
    },
    AgentMerge {
        agent_id: String,
        target: String,
    },
    /// Push a Merge whose push failed, again.
    PushMerge {
        agent_id: String,
    },
    /// Spawn a Resolver for a Merge of the Agent into `target` that conflicted.
    ResolveConflict {
        agent_id: String,
        target: String,
        files: Vec<String>,
        model: Option<String>,
        effort: Option<String>,
    },
    AgentsHoldingWork {},
    ListModels {},
    SlashCommands {
        dir: PathBuf,
    },
    UsageSummary {},
    StartupOrphans {},
    DismissOrphans {},
    /// Exit as soon as no Turn is running, so the service can start the build
    /// the asking window came from. Kept, with this name and no arguments, in
    /// every protocol version: it is how a window that can't talk to an older
    /// or newer Host gets it out of the way.
    ShutdownWhenIdle {},
}

/// How a Merge the user asked for came out. A conflict is an outcome rather
/// than an error: the Project is untouched, and the UI offers to Resolve it.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MergeOutcome {
    Merged(Merged),
    Conflict { target: String, files: Vec<String> },
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn ok<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(err)
}

/// Answer one call from its raw JSON.
pub async fn answer(host: &Host, call: Value) -> Result<Value, String> {
    let call: Call =
        serde_json::from_value(call).map_err(|e| format!("the Host can't read this call: {e}"))?;
    handle(host, call).await
}

async fn handle(host: &Host, call: Call) -> Result<Value, String> {
    let runtime = &host.runtime;
    match call {
        Call::ListProjects {} => ok(storage::Registry::load().map_err(err)?.projects),

        Call::ProjectNeedsSetup { path } => ok(!git::has_commits(&path).await),

        Call::AddProject { name, path, set_up } => {
            if !git::has_commits(&path).await {
                if !set_up {
                    return Err(err(Error::NotSetUp { path }));
                }
                git::set_up(&path).await.map_err(err)?;
            }
            let mut reg = storage::Registry::load().map_err(err)?;
            let project = Project {
                id: new_id(),
                name,
                path,
                added_at: OffsetDateTime::now_utc(),
            };
            reg.projects.push(project.clone());
            reg.save().map_err(err)?;
            ok(project)
        }

        Call::RemoveProject { id } => {
            let mut reg = storage::Registry::load().map_err(err)?;
            reg.projects.retain(|p| p.id != id);
            ok(reg.save().map_err(err)?)
        }

        Call::ListAgents {} => ok(storage::list_agents().map_err(err)?),

        Call::SpawnAgent {
            project_id,
            prompt,
            attachments,
            model,
            effort,
            permission_mode,
            options,
        } => {
            let reg = storage::Registry::load().map_err(err)?;
            let project = reg
                .project(&project_id)
                .ok_or_else(|| format!("project not found: {project_id}"))?;
            let agent = runtime
                .spawn(
                    project,
                    prompt,
                    attachments,
                    model,
                    effort,
                    permission_mode,
                    options,
                )
                .await
                .map_err(err)?;
            ok(agent)
        }

        Call::SendMessage {
            agent_id,
            prompt,
            attachments,
            model,
            effort,
            permission_mode,
        } => {
            let message = QueuedMessage {
                id: new_id(),
                prompt: prompt.trim().to_string(),
                attachments,
                model,
                effort,
                permission_mode,
            };
            ok(runtime.send(&agent_id, message).await.map_err(err)?)
        }

        Call::SendNext { agent_id } => ok(runtime.send_next(&agent_id).await.map_err(err)?),

        Call::QueueMessages { agent_id, messages } => ok(runtime
            .edit(&agent_id, |a| a.queue.extend(messages))
            .await
            .map_err(err)?),

        Call::RemoveQueued {
            agent_id,
            message_id,
        } => ok(runtime
            .edit(&agent_id, |a| a.queue.retain(|m| m.id != message_id))
            .await
            .map_err(err)?),

        Call::ClearQueue { agent_id } => ok(runtime
            .edit(&agent_id, |a| a.queue.clear())
            .await
            .map_err(err)?),

        Call::StoreAttachment { name, data } => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| format!("the attachment {name} didn't arrive intact: {e}"))?;
            ok(crate::attachments::save(&name, &bytes).map_err(err)?)
        }

        Call::KeepRunning {} => ok(tokio::task::spawn_blocking(super::service::keeps_running)
            .await
            .map_err(err)?),

        Call::SetKeepRunning { on } => {
            tokio::task::spawn_blocking(move || super::service::set_keeps_running(on))
                .await
                .map_err(err)?
                .map_err(|e| format!("could not change it: {e}"))?;
            ok(())
        }

        Call::RenameAgent { agent_id, name } => {
            let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
            ok(runtime
                .edit(&agent_id, |a| a.user_title = name)
                .await
                .map_err(err)?)
        }

        Call::SetAgentColor { agent_id, color } => ok(runtime
            .edit(&agent_id, |a| a.color = color)
            .await
            .map_err(err)?),

        Call::SetAgentOptions { agent_id, options } => ok(runtime
            .edit(&agent_id, |a| a.options = options)
            .await
            .map_err(err)?),

        Call::StopAgent { agent_id } => ok(runtime.stop(&agent_id).await.map_err(err)?),

        Call::DiscardAgent { agent_id } => ok(discard(&agent_id).await?),

        Call::AgentEvents { agent_id } => ok(storage::read_events(&agent_id).map_err(err)?),

        Call::AgentDiff { agent_id } => {
            let agent = storage::load_agent(&agent_id).map_err(err)?;
            let reg = storage::Registry::load().map_err(err)?;
            let project_path = reg.project(&agent.project_id).map(|p| p.path.clone());
            ok(git::diff(
                project_path.as_deref(),
                &agent.worktree_path,
                &agent.branch,
                agent.base_commit.as_deref(),
            )
            .await
            .map_err(err)?)
        }

        Call::AgentCommit { agent_id, message } => {
            // Refused while the Agent is running, since it would race the
            // Agent's own writes and capture a half-finished tree.
            let agent = storage::load_agent(&agent_id).map_err(err)?;
            if agent.state == AgentState::Running {
                return Err("cannot commit while the agent is running; stop it first".into());
            }
            ok(git::commit(&agent.worktree_path, &message)
                .await
                .map_err(err)?)
        }

        Call::ProjectBranches { project_id } => {
            let reg = storage::Registry::load().map_err(err)?;
            let project = reg
                .project(&project_id)
                .ok_or_else(|| format!("project not found: {project_id}"))?;
            ok(git::branches(&project.path).await.map_err(err)?)
        }

        Call::AgentMerge { agent_id, target } => {
            // The one thing the app writes to the Project, and only ever
            // because the user asked. The Agent survives a Merge: only Discard
            // destroys anything, so the Merge is recorded on the Agent.
            let mut agent = storage::load_agent(&agent_id).map_err(err)?;
            if agent.state == AgentState::Running {
                return Err("cannot merge while the agent is running; stop it first".into());
            }
            let project = project_of(&agent, "merge")?;
            match merging::merge(&project.path, &mut agent, &target).await {
                Ok((merged, _)) => ok(MergeOutcome::Merged(merged)),
                Err(Error::MergeConflict { target, files }) => {
                    ok(MergeOutcome::Conflict { target, files })
                }
                Err(e) => Err(err(e)),
            }
        }

        Call::PushMerge { agent_id } => {
            let agent = storage::load_agent(&agent_id).map_err(err)?;
            let project = project_of(&agent, "push")?;
            let why = merging::push_again(&project.path, &agent)
                .await
                .map_err(err)?;
            ok(runtime
                .edit(&agent_id, |a| a.push_error = why)
                .await
                .map_err(err)?)
        }

        Call::ResolveConflict {
            agent_id,
            target,
            files,
            model,
            effort,
        } => {
            // Refused while the Agent is running, as a Merge is — the Resolver
            // would be cut from a branch still moving.
            let agent = storage::load_agent(&agent_id).map_err(err)?;
            if agent.state == AgentState::Running {
                return Err("cannot resolve while the agent is running; stop it first".into());
            }
            let project = project_of(&agent, "resolve")?;
            ok(runtime
                .spawn_resolver(&project, &agent, &target, &files, model, effort)
                .await
                .map_err(err)?)
        }

        Call::AgentsHoldingWork {} => ok(agents_holding_work().await?),

        // Asked of Anthropic rather than hardcoded, so the list matches the
        // account and picks up models released after this build.
        Call::ListModels {} => ok(crate::models::list().await.map_err(err)?),

        Call::SlashCommands { dir } => ok(crate::slash::list(runtime.claude_bin(), &dir)
            .await
            .map_err(err)?),

        // Off the async runtime: the first read walks every Claude Code
        // transcript.
        Call::UsageSummary {} => ok(tokio::task::spawn_blocking(crate::usage::summary)
            .await
            .map_err(err)?
            .map_err(err)?),

        // Read fresh from disk, so one the user has since Resumed or Discarded
        // drops out.
        Call::StartupOrphans {} => {
            let ids = host.startup_orphans.lock().map_err(err)?.clone();
            ok(ids
                .iter()
                .filter_map(|id| storage::load_agent(id).ok())
                .filter(|a| a.state == AgentState::Orphaned)
                .collect::<Vec<Agent>>())
        }

        Call::DismissOrphans {} => {
            host.startup_orphans.lock().map_err(err)?.clear();
            ok(())
        }

        Call::ShutdownWhenIdle {} => {
            host.shut_down_when_idle();
            ok(())
        }
    }
}

async fn discard(agent_id: &str) -> Result<(), String> {
    let agent = storage::load_agent(agent_id).map_err(err)?;
    if agent.state == AgentState::Running {
        return Err("cannot discard a running agent; stop it first".into());
    }

    // Best-effort worktree removal. If the Project has been unregistered we
    // fall back to a plain directory delete.
    let reg = storage::Registry::load().map_err(err)?;
    if let Some(project) = reg.project(&agent.project_id) {
        let _ = worktree::discard(&project.path, &agent.worktree_path, &agent.branch).await;
    } else if agent.worktree_path.exists() {
        let _ = std::fs::remove_dir_all(&agent.worktree_path);
    }

    // Delete the meta file so the Agent disappears from list_agents.
    let meta = paths::agent_meta_path(agent_id).map_err(err)?;
    if meta.exists() {
        std::fs::remove_file(&meta).map_err(err)?;
    }
    Ok(())
}

/// The registered Project an Agent belongs to, or an error naming `action` for
/// an Agent whose Project has since been removed.
fn project_of(agent: &Agent, action: &str) -> Result<Project, String> {
    let reg = storage::Registry::load().map_err(err)?;
    reg.project(&agent.project_id).cloned().ok_or_else(|| {
        format!(
            "cannot {action}: the project this agent belongs to is no longer registered ({})",
            agent.project_id
        )
    })
}

/// The Merged Agents whose Worktree still holds work the Project does not
/// have — left dirty, or committed again after the Merge.
///
/// The sidebar buckets on the merge record alone, which would leave an Agent
/// that was Resumed and did more work sitting in Delivered as though you were
/// done with it. This is the one extra fact that reading needs, and it is asked
/// only of Agents that have a merge record at all, so the git cost stays
/// bounded by how many Agents you have already Merged rather than by how many
/// you have. Running Agents are skipped: their Worktree is being written as we
/// look, and Running outranks the merge record anyway.
async fn agents_holding_work() -> Result<Vec<String>, String> {
    let agents = storage::list_agents().map_err(err)?;
    let mut holding = Vec::new();
    for agent in agents {
        if agent.merged_at.is_none() || agent.state == AgentState::Running {
            continue;
        }
        if git::holds_unmerged_work(&agent.worktree_path).await {
            holding.push(agent.id);
        }
    }
    Ok(holding)
}
