//! Handing an Agent's work off to another Host (ADR 0013).
//!
//! An Agent can't leave its Host: its Session and Worktree live on that disk.
//! What can move is its work. The Agent's Host pushes the Agent's branch to the
//! Project's remote under a ref of its own, and writes a brief of what the
//! Agent was doing; the chosen Host Spawns a new Agent from that ref, with the
//! brief ahead of the user's prompt, and takes the ref down again.

use serde::{Deserialize, Serialize};

use crate::domain::{new_id, Agent, AgentState, Project};
use crate::error::{Error, Result};
use crate::{git, storage};

/// Longest stretch of the Agent's last answer carried in a brief. Enough for a
/// summary of where it got to; a whole transcript would drown the new prompt.
const ANSWER_EXCERPT: usize = 4000;

/// What travels from the Agent's Host to the one picking its work up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    /// The branch on the remote the work is waiting on.
    pub branch: String,
    /// The Agent's Base, so the new Agent's diff reads as the whole of the
    /// work rather than only what it adds.
    pub base_commit: Option<String>,
    /// What the new Agent is told about the work it picks up, ahead of the
    /// user's prompt.
    pub brief: String,
    /// The Agent's Title, for the new one to go by until Claude names it:
    /// otherwise every Handoff would be listed under the brief's first line.
    #[serde(default)]
    pub title: Option<String>,
}

/// Put `agent`'s committed work on the remote for another Host to pick up.
///
/// Refused while the Agent works, since its branch is still moving, and while
/// its Worktree holds anything uncommitted, which the new Agent would silently
/// go without.
pub async fn hand_off(project: &Project, agent: &Agent, machine: &str) -> Result<Handoff> {
    if agent.state == AgentState::Running {
        return Err(Error::AgentBusy(agent.id.clone()));
    }
    if agent.worktree_path.exists() && git::is_dirty(&agent.worktree_path).await? {
        return Err(Error::HandoffDirty {
            path: agent.worktree_path.clone(),
        });
    }
    let branch = git::handoff_branch(&new_id());
    git::publish(&project.path, &agent.branch, &branch).await?;
    let answer = last_answer(&agent.id);
    Ok(Handoff {
        brief: brief(agent, &answer, machine),
        branch,
        base_commit: agent.base_commit.clone(),
        title: agent.user_title.clone().or_else(|| agent.title.clone()),
    })
}

/// The Task a new Agent picking up a Handoff is given: the brief, then what
/// the user said to do next.
pub fn task(handoff: &Handoff, prompt: &str) -> String {
    format!("{}\n\n{}", handoff.brief, prompt.trim())
}

/// The final answer of the Agent's last Turn that gave one, from its log.
fn last_answer(agent_id: &str) -> String {
    storage::read_events(agent_id)
        .unwrap_or_default()
        .iter()
        .rev()
        .find_map(|e| {
            (e.event.get("type")?.as_str()? == "result")
                .then(|| e.event.get("result")?.as_str())
                .flatten()
                .filter(|r| !r.trim().is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn quote(text: &str) -> String {
    format!("> {}", text.trim().replace('\n', "\n> "))
}

/// What the new Agent is told about the work it picks up.
pub fn brief(agent: &Agent, answer: &str, machine: &str) -> String {
    let base = agent
        .base_commit
        .as_deref()
        .map(|b| {
            format!(
                " Its work started from `{short}`, so `git log {short}..HEAD` lists what \
                 it committed.",
                short = &b[..b.len().min(12)]
            )
        })
        .unwrap_or_default();
    let mut brief = format!(
        "You're picking up another agent's work. It ran on {machine}; this worktree starts \
         where it left off, with everything it committed.{base}\n\
         \n\
         Its task was:\n\
         \n\
         {task}\n",
        task = quote(&agent.task.prompt),
    );
    if !answer.trim().is_empty() {
        let excerpt: String = answer.trim().chars().take(ANSWER_EXCERPT).collect();
        let cut = if excerpt.len() < answer.trim().len() {
            "\n> …"
        } else {
            ""
        };
        brief.push_str(&format!(
            "\nIts last answer was:\n\n{}{cut}\n",
            quote(&excerpt)
        ));
    }
    brief.push_str("\nWhat to do next:");
    brief
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentOptions, Task};
    use time::OffsetDateTime;

    fn agent(prompt: &str, base: Option<&str>) -> Agent {
        Agent {
            id: "a".into(),
            project_id: "p".into(),
            task: Task {
                prompt: prompt.into(),
                attachments: vec![],
            },
            state: AgentState::Completed,
            worktree_path: "/nowhere".into(),
            branch: "cw/agent-a".into(),
            base_commit: base.map(str::to_owned),
            session_id: None,
            model: None,
            effort: None,
            permission_mode: None,
            options: AgentOptions::default(),
            turns: 1,
            title: None,
            user_title: None,
            color: None,
            spawned_at: OffsetDateTime::UNIX_EPOCH,
            turn_started_at: None,
            exited_at: None,
            exit_code: None,
            fail_reason: None,
            merged_branch: None,
            merged_at: None,
            unpushed: false,
            push_error: None,
            resolves: None,
            queue: vec![],
        }
    }

    #[test]
    fn a_brief_carries_the_task_the_answer_and_where_the_work_starts() {
        let a = agent("Fix the login\nand the logout", Some("0123456789abcdef"));
        let b = brief(&a, "Fixed both.", "desk");
        assert!(b.contains("It ran on desk"));
        assert!(b.contains("`git log 0123456789ab..HEAD`"));
        assert!(b.contains("> Fix the login\n> and the logout"));
        assert!(b.contains("> Fixed both."));
        assert!(b.ends_with("What to do next:"));
    }

    #[test]
    fn a_brief_without_an_answer_or_a_base_leaves_them_out() {
        let b = brief(&agent("Fix it", None), "", "desk");
        assert!(!b.contains("last answer"));
        assert!(!b.contains("git log"));
    }

    #[test]
    fn a_long_answer_is_cut_and_says_so() {
        let long = "x".repeat(ANSWER_EXCERPT + 10);
        let b = brief(&agent("Fix it", None), &long, "desk");
        assert!(b.contains("\n> …"));
        assert!(!b.contains(&long));
    }

    #[test]
    fn the_task_puts_the_prompt_after_the_brief() {
        let h = Handoff {
            branch: "cw/handoff/x".into(),
            base_commit: None,
            brief: "Brief.\n\nWhat to do next:".into(),
            title: None,
        };
        assert_eq!(
            task(&h, "  Carry on  "),
            "Brief.\n\nWhat to do next:\n\nCarry on"
        );
    }
}
