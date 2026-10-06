//! The Agents Space's MCP tools (see `crate::mcp` for how tools work). Any
//! client may read the Agents. An Agent may also lead (ADR 0019): Spawn
//! Helpers, wait for them, send them follow-ups and Stop them, acting only on
//! its own. Nothing here Merges. A Mail-locked Agent's Task, Title and answers
//! are never given out (ADR 0018): they come from mail, and the Agent asking
//! may not be locked.

use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::domain::{Agent, AgentState, Project};
use crate::mcp::{Host, NoArgs, Tool};

/// The tools a Lead uses on its Helpers. A Helper's Turns aren't offered
/// them, since a Helper can't lead.
pub const LEAD_TOOLS: [&str; 4] = [
    "spawn_helper",
    "wait_for_helpers",
    "send_to_helper",
    "stop_helper",
];

/// How often `wait_for_helpers` looks again while a Helper is working.
const WAIT_POLL: Duration = Duration::from_secs(2);

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
        Tool::writes(
            "spawn_helper",
            "Start a Helper: another Orchestrate Agent that works on one part of your task \
             while you and other Helpers work on others. Use it to split work that can go \
             in parallel. Each Helper gets its own Git worktree, on a branch cut from your \
             branch's latest commit, so commit first anything it should build on, and give \
             each one a piece that doesn't touch the same files as another's. It runs in \
             your permission mode, commits its work on its own branch, and shows under you \
             in the user's sidebar. Then call wait_for_helpers, and merge each finished \
             Helper's branch into yours with `git merge <branch>`. At most 8 work at once.",
            spawn_helper,
        ),
        Tool::reads(
            "wait_for_helpers",
            "Wait until your Helpers have finished their Turns, then report each one: its \
             State, branch, worktree and final answer. Waits for all of them unless you \
             name some. After `minutes` it reports anyway, with the ones still working \
             marked running, and you can call it again.",
            wait_for_helpers,
        ),
        Tool::writes(
            "send_to_helper",
            "Give one of your Helpers a follow-up, to correct it or hand it more: it starts \
             a new Turn now if it has finished, or as soon as its current one ends. Then \
             wait_for_helpers again.",
            send_to_helper,
        ),
        Tool::writes(
            "stop_helper",
            "Stop one of your Helpers mid-Turn. Its work so far stays in its worktree, and \
             send_to_helper sets it going again.",
            stop_helper,
        ),
    ]
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NewHelper {
    /// What the Helper is to do, complete in itself: it sees none of your
    /// conversation, only this and the code at your branch's latest commit.
    task: String,
    /// The model it runs on, as `claude --model` takes it: an alias such as
    /// `sonnet`, or a full model name. Leave it out to use yours.
    model: Option<String>,
    /// How hard it works: `low`, `medium`, `high`, `xhigh` or `max`. Leave it
    /// out to use yours.
    effort: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Waiting {
    /// Which of your Helpers to wait for, by id or Title. Leave it out to wait
    /// for all of them.
    helpers: Option<Vec<String>>,
    /// How long to wait before reporting anyway, in minutes: 10 unless you say,
    /// and at most 30.
    minutes: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FollowUp {
    /// The Helper's id, as spawn_helper gave it, or its exact Title.
    helper: String,
    /// What to tell it.
    message: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WhichHelper {
    /// The Helper's id, as spawn_helper gave it, or its exact Title.
    helper: String,
}

/// A Helper as `spawn_helper` reports it.
#[derive(Serialize)]
struct Spawned {
    id: String,
    branch: String,
    worktree: PathBuf,
}

/// A Helper as `wait_for_helpers` reports it.
#[derive(Serialize)]
struct Report {
    id: String,
    title: String,
    state: AgentState,
    branch: String,
    worktree: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    fail_reason: Option<String>,
    /// The final answer of its last Turn, once it isn't working.
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<String>,
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
    /// The Lead that Spawned it, for a Helper.
    #[serde(skip_serializing_if = "Option::is_none")]
    lead: Option<String>,
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
            lead: a.lead_id.clone(),
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
    let listed = Listed::of(&a, &projects, &host);
    let permission_mode = a.mode().to_string();
    let task = if a.read_mail {
        HELD_BACK.into()
    } else {
        a.task.prompt
    };
    Ok(Status {
        listed,
        task,
        turns: a.turns,
        model: a.model,
        effort: a.effort,
        permission_mode,
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
    if a.read_mail {
        return Err(format!(
            "“{}” was handed mail, so what it says is shown only to the user, in the app.",
            title(a)
        ));
    }
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

async fn spawn_helper(host: Host, new: NewHelper) -> Result<Spawned, String> {
    let helper: Agent = host
        .call_as(
            "spawn_helper",
            json!({
                "leadId": host.agent()?,
                "prompt": new.task,
                "model": new.model,
                "effort": new.effort,
            }),
        )
        .await?;
    Ok(Spawned {
        id: helper.id,
        branch: helper.branch,
        worktree: helper.worktree_path,
    })
}

async fn wait_for_helpers(host: Host, waiting: Waiting) -> Result<Vec<Report>, String> {
    let lead = host.agent()?.to_owned();
    let patience = Duration::from_secs(60 * waiting.minutes.unwrap_or(10).clamp(1, 30));
    let until = tokio::time::Instant::now() + patience;
    loop {
        let agents: Vec<Agent> = host.call_as("list_agents", json!({})).await?;
        let helpers = helpers_of(&agents, &lead);
        if helpers.is_empty() {
            return Err("you have no Helpers; spawn_helper starts one".into());
        }
        let waited_for = match &waiting.helpers {
            None => helpers,
            Some(keys) => keys
                .iter()
                .map(|key| own_helper(&helpers, key).cloned())
                .collect::<Result<_, _>>()?,
        };
        let working = waited_for.iter().any(|a| a.state == AgentState::Running);
        if !working || tokio::time::Instant::now() >= until {
            let mut reports = Vec::with_capacity(waited_for.len());
            for a in waited_for {
                reports.push(report(&host, a).await?);
            }
            return Ok(reports);
        }
        tokio::time::sleep(WAIT_POLL).await;
    }
}

async fn report(host: &Host, a: Agent) -> Result<Report, String> {
    // A Helper is never Mail-locked, since its Lead can't be; held back all
    // the same, as every tool holds back a locked Agent's answers.
    let answer = if a.state == AgentState::Running || a.read_mail {
        None
    } else {
        host.call_as("agent_last_answer", json!({ "agentId": a.id }))
            .await?
    };
    Ok(Report {
        title: title(&a),
        id: a.id,
        state: a.state,
        branch: a.branch,
        worktree: a.worktree_path,
        fail_reason: a.fail_reason,
        answer,
    })
}

async fn send_to_helper(host: Host, follow_up: FollowUp) -> Result<String, String> {
    let lead = host.agent()?;
    let agents: Vec<Agent> = host.call_as("list_agents", json!({})).await?;
    let helper = own_helper(&helpers_of(&agents, lead), &follow_up.helper)?.clone();
    let sent: Agent = host
        .call_as(
            "send_to_helper",
            json!({ "leadId": lead, "helperId": helper.id, "prompt": follow_up.message }),
        )
        .await?;
    Ok(if sent.queue.is_empty() {
        format!("“{}” is working on it.", title(&helper))
    } else {
        format!(
            "“{}” is still working, so it gets this as soon as its current Turn ends.",
            title(&helper)
        )
    })
}

async fn stop_helper(host: Host, which: WhichHelper) -> Result<String, String> {
    let lead = host.agent()?;
    let agents: Vec<Agent> = host.call_as("list_agents", json!({})).await?;
    let helper = own_helper(&helpers_of(&agents, lead), &which.helper)?.clone();
    if helper.state != AgentState::Running {
        return Ok(format!("“{}” wasn't working.", title(&helper)));
    }
    host.call(
        "stop_helper",
        json!({ "leadId": lead, "helperId": helper.id }),
    )
    .await?;
    Ok(format!(
        "Stopped “{}”. Its work so far is in {}.",
        title(&helper),
        helper.worktree_path.display()
    ))
}

/// The Helpers `lead` Spawned, oldest first.
fn helpers_of(agents: &[Agent], lead: &str) -> Vec<Agent> {
    let mut helpers: Vec<Agent> = agents
        .iter()
        .filter(|a| a.lead_id.as_deref() == Some(lead))
        .cloned()
        .collect();
    helpers.sort_by_key(|a| a.spawned_at);
    helpers
}

/// The one of `helpers` that `key` names, by id or else by Title.
fn own_helper<'a>(helpers: &'a [Agent], key: &str) -> Result<&'a Agent, String> {
    find_in(helpers, key, || {
        format!("none of your Helpers has the id or Title “{}”.", key.trim())
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
    find_in(agents, key, || {
        format!(
            "no Agent on this Host has the id or Title “{}”. list_agents names them all.",
            key.trim()
        )
    })
}

/// The one of `agents` that `key` names, by id or else by Title, or `missing`
/// when none does.
fn find_in<'a>(
    agents: &'a [Agent],
    key: &str,
    missing: impl FnOnce() -> String,
) -> Result<&'a Agent, String> {
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
        [] => Err(missing()),
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

/// What a Mail-locked Agent's Task reads as over MCP. Its Task, Title and
/// answers are drawn from mail, which must not reach an Agent that isn't
/// locked (ADR 0018); tools are how one Agent reads another.
const HELD_BACK: &str = "(held back: this Agent was handed mail)";

/// The Title the app shows: the user's, else Claude's, else the Task's first
/// line until the first Turn names it. Only the user's for a Mail-locked
/// Agent, since Claude's is drawn from its mail.
fn title(a: &Agent) -> String {
    if a.read_mail {
        return a
            .user_title
            .clone()
            .unwrap_or_else(|| "An Agent that was handed mail".into());
    }
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
