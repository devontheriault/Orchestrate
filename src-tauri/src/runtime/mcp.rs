//! The Agents Space's MCP tools (see `crate::mcp` for how tools work). They
//! only read. Spawning, sending to and Merging an Agent over MCP wait on a
//! decision of their own.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::{Agent, AgentState, Project, DEFAULT_PERMISSION_MODE};
use crate::mcp::{Host, NoArgs, Tool};

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::reads(
            "list_agents",
            "List the user's Orchestrate Agents on this machine, newest first: each one's \
             id, Title, State (running, completed, failed, stopped or orphaned), Project \
             and Host. An Agent is a Claude Code session working in its own Git worktree.",
            list_agents,
        ),
        Tool::reads(
            "agent_status",
            "One Orchestrate Agent in detail: its Task, State, Turns, model, branch, \
             worktree, when it last ran, why it failed if it did, and where its work was \
             merged.",
            agent_status,
        ),
        Tool::reads(
            "agent_last_answer",
            "The final answer an Orchestrate Agent gave at the end of its last Turn: \
             what it reported back about its work.",
            agent_last_answer,
        ),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WhichAgent {
    /// The Agent's id, as list_agents gives it, or its exact Title.
    agent: String,
}

/// An Agent as `list_agents` gives it.
#[derive(Serialize)]
struct Listed {
    id: String,
    title: String,
    state: AgentState,
    project: Option<String>,
    host: String,
}

impl Listed {
    fn of(a: &Agent, projects: &[Project], host: &Host) -> Self {
        Self {
            id: a.id.clone(),
            title: title(a),
            state: a.state,
            project: projects
                .iter()
                .find(|p| p.id == a.project_id)
                .map(|p| p.name.clone()),
            host: host.name().to_owned(),
        }
    }
}

/// An Agent as `agent_status` describes it.
#[derive(Serialize)]
struct Status {
    #[serde(flatten)]
    listed: Listed,
    task: String,
    turns: u32,
    model: Option<String>,
    effort: Option<String>,
    permission_mode: String,
    branch: String,
    worktree: PathBuf,
    base_commit: Option<String>,
    spawned_at: Option<String>,
    turn_started_at: Option<String>,
    exited_at: Option<String>,
    fail_reason: Option<String>,
    merged_into: Option<Merged>,
    queued_messages: usize,
}

#[derive(Serialize)]
struct Merged {
    branch: String,
    at: Option<String>,
    pushed: bool,
}

async fn list_agents(host: Host, _: NoArgs) -> Result<Vec<Listed>, String> {
    let (mut agents, projects) = read(&host).await?;
    agents.sort_by_key(|a| std::cmp::Reverse(a.spawned_at));
    Ok(agents
        .iter()
        .map(|a| Listed::of(a, &projects, &host))
        .collect())
}

async fn agent_status(host: Host, which: WhichAgent) -> Result<Status, String> {
    let (agents, projects) = read(&host).await?;
    let a = find(&agents, &which.agent)?.clone();
    Ok(Status {
        listed: Listed::of(&a, &projects, &host),
        task: a.task.prompt,
        turns: a.turns,
        model: a.model,
        effort: a.effort,
        permission_mode: a
            .permission_mode
            .unwrap_or_else(|| DEFAULT_PERMISSION_MODE.into()),
        branch: a.branch,
        worktree: a.worktree_path,
        base_commit: a.base_commit,
        spawned_at: when(Some(a.spawned_at)),
        turn_started_at: when(a.turn_started_at),
        exited_at: when(a.exited_at),
        fail_reason: a.fail_reason,
        merged_into: a.merged_branch.map(|branch| Merged {
            branch,
            at: when(a.merged_at),
            pushed: !a.unpushed,
        }),
        queued_messages: a.queue.len(),
    })
}

async fn agent_last_answer(host: Host, which: WhichAgent) -> Result<String, String> {
    let (agents, _) = read(&host).await?;
    let a = find(&agents, &which.agent)?;
    let answer: Option<String> = host
        .call_as("agent_last_answer", json!({ "agentId": a.id }))
        .await?;
    Ok(match answer {
        None => format!("“{}” hasn't finished a Turn with an answer yet.", title(a)),
        Some(answer) if a.state == AgentState::Running => format!(
            "“{}” is still working on a Turn. This is the answer from the one before:\n\n{answer}",
            title(a)
        ),
        Some(answer) => answer,
    })
}

/// Every Agent on the Host, and its Projects to name them by.
async fn read(host: &Host) -> Result<(Vec<Agent>, Vec<Project>), String> {
    let agents = host.call_as("list_agents", json!({}));
    let projects = host.call_as("list_projects", json!({}));
    tokio::try_join!(agents, projects)
}

/// The Agent `key` names, by id or else by Title.
fn find<'a>(agents: &'a [Agent], key: &str) -> Result<&'a Agent, String> {
    let key = key.trim();
    if let Some(a) = agents.iter().find(|a| a.id == key) {
        return Ok(a);
    }
    let named: Vec<&Agent> = agents
        .iter()
        .filter(|a| title(a).eq_ignore_ascii_case(key))
        .collect();
    match named[..] {
        [a] => Ok(a),
        [] => Err(format!(
            "no Agent on this Host has the id or Title “{key}”. list_agents names them all."
        )),
        _ => Err(format!(
            "{} Agents are called “{key}”; ask for one by id: {}.",
            named.len(),
            named
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The Title the app shows: the user's, else Claude's, else the Task's first
/// line until the first Turn names it.
fn title(a: &Agent) -> String {
    a.user_title
        .clone()
        .or_else(|| a.title.clone())
        .unwrap_or_else(|| {
            let first = a.task.prompt.trim().lines().next().unwrap_or_default();
            match first.char_indices().nth(80) {
                Some((cut, _)) => format!("{}…", &first[..cut]),
                None => first.to_string(),
            }
        })
}

fn when(t: Option<OffsetDateTime>) -> Option<String> {
    t.and_then(|t| t.format(&Rfc3339).ok())
}
