use super::*;
use crate::domain::AgentEvent;
use crate::test_util::{init_repo, write_script, StateEnv};
use tempfile::TempDir;
use tokio::process::Command;

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

#[tokio::test]
async fn spawn_run_to_completion() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let bin = fake_claude_ok();
    let (rt, mut rx) = AgentRuntime::with_bin(bin);

    let agent = rt
        .spawn(
            &project,
            "hello".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.state, AgentState::Running);

    // Consume events until we see StateChanged (final).
    let final_agent = loop {
        match rx.recv().await.expect("channel open") {
            RuntimeEvent::AgentEvent { .. } => continue,
            RuntimeEvent::StateChanged { agent, .. } => break *agent,
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

    let agent = rt
        .spawn(
            &project,
            "write some code".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let base = agent
        .base_commit
        .clone()
        .expect("spawn records the commit it branched from");

    let final_agent = loop {
        match rx.recv().await.expect("channel open") {
            RuntimeEvent::AgentEvent { .. } => continue,
            RuntimeEvent::StateChanged { agent, .. } => break *agent,
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

    let agent = rt
        .spawn(
            &project,
            "boom".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let final_agent = loop {
        if let RuntimeEvent::StateChanged { agent, .. } = rx.recv().await.unwrap() {
            break *agent;
        }
    };
    assert_eq!(final_agent.state, AgentState::Failed);
    assert_eq!(final_agent.exit_code, Some(42));
    assert!(
        final_agent
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("boom"),
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

    let agent = rt
        .spawn(
            &project,
            "hang".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
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
        "Stop leaves the worktree; Discard is the explicit action that removes it"
    );
}

/// Waits for the Turn to end — skipping the `Running` announcement a Resume
/// makes on its way in.
async fn wait_for_exit(rx: &mut mpsc::UnboundedReceiver<RuntimeEvent>) -> Agent {
    loop {
        match rx.recv().await.expect("channel open") {
            RuntimeEvent::StateChanged { agent, .. } if agent.state != AgentState::Running => {
                break *agent
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

    let agent = rt
        .spawn(
            &project,
            "first".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let session = agent.session_id.clone().expect("spawn mints a session");
    let after_first = wait_for_exit(&mut rx).await;
    assert_eq!(after_first.state, AgentState::Completed);
    assert_eq!(after_first.turns, 1);

    let resumed = rt
        .resume(&agent.id, "second".into(), vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(resumed.state, AgentState::Running);
    assert_eq!(
        resumed.turns, 2,
        "a follow-up is a new Turn on the same Agent"
    );
    assert_eq!(resumed.worktree_path, agent.worktree_path, "same sandbox");
    assert_eq!(resumed.session_id.as_deref(), Some(session.as_str()));

    let after_second = wait_for_exit(&mut rx).await;
    assert_eq!(after_second.state, AgentState::Completed);
    assert_eq!(after_second.turns, 2);

    // The opening Turn started the session; the follow-up resumed it.
    let args = std::fs::read_to_string(&args_log).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 2, "one `claude` per Turn: {args}");
    assert!(
        lines[0].contains(&format!("--session-id {session}")),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].contains(&format!("--resume {session}")),
        "{}",
        lines[1]
    );
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
        .spawn(
            &project,
            "first".into(),
            vec![],
            Some("sonnet".into()),
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.model.as_deref(), Some("sonnet"));
    wait_for_exit(&mut rx).await;

    // A user who starts cheap can escalate without starting over.
    let resumed = rt
        .resume(
            &agent.id,
            "try harder".into(),
            vec![],
            Some("opus".into()),
            None,
            None,
        )
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

    let agent = rt
        .spawn(
            &project,
            "hello".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.model, None);
    assert_eq!(agent.effort, None);
    wait_for_exit(&mut rx).await;

    let args = std::fs::read_to_string(&args_log).unwrap();
    assert!(!args.contains("--model"), "{args}");
    assert!(!args.contains("--effort"), "{args}");
}

/// `--print` kills background shells once it has answered, so a Turn that
/// could background its work would be marked Completed with it unfinished.
#[tokio::test]
async fn every_turn_runs_with_background_tasks_off() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let env_log = std::env::temp_dir().join(format!("cw-env-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(write_script(&format!(
        r#"#!/bin/sh
echo "bg=$CLAUDE_CODE_DISABLE_BACKGROUND_TASKS" >> {log}
exit 0
"#,
        log = env_log.display()
    )));

    let agent = rt
        .spawn(
            &project,
            "first".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;
    rt.resume(&agent.id, "second".into(), vec![], None, None, None)
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;

    let seen = std::fs::read_to_string(&env_log).unwrap();
    assert_eq!(seen, "bg=1\nbg=1\n");
}

/// Effort rides along with the model: picked per Turn, recorded on the
/// Agent, and passed to `claude` as its own flag.
#[tokio::test]
async fn the_picked_effort_is_passed_to_claude_and_carried_across_turns() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

    let agent = rt
        .spawn(
            &project,
            "first".into(),
            vec![],
            None,
            Some("low".into()),
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.effort.as_deref(), Some("low"));
    wait_for_exit(&mut rx).await;

    let resumed = rt
        .resume(
            &agent.id,
            "think harder".into(),
            vec![],
            None,
            Some("max".into()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(resumed.effort.as_deref(), Some("max"));
    wait_for_exit(&mut rx).await;
    assert_eq!(
        storage::load_agent(&agent.id).unwrap().effort.as_deref(),
        Some("max"),
        "the new choice must be persisted, or the next Turn reverts"
    );

    let args = std::fs::read_to_string(&args_log).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 2, "one `claude` per Turn: {args}");
    assert!(lines[0].contains("--effort low"), "{}", lines[0]);
    assert!(lines[1].contains("--effort max"), "{}", lines[1]);
}

/// The Mode is the Agent's, picked per Turn: a Turn that plans first and a
/// Turn that acts are the same Agent in the same Worktree, one flag apart.
/// Unlike the model and effort flags, this one is always passed — an Agent
/// with no pick still gets the Mode it has always run in.
#[tokio::test]
async fn the_picked_mode_is_passed_to_claude_and_carried_across_turns() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

    let agent = rt
        .spawn(
            &project,
            "plan it".into(),
            vec![],
            None,
            None,
            Some("plan".into()),
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.permission_mode.as_deref(), Some("plan"));
    wait_for_exit(&mut rx).await;

    let resumed = rt
        .resume(
            &agent.id,
            "now build it".into(),
            vec![],
            None,
            None,
            Some("bypassPermissions".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        resumed.permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    wait_for_exit(&mut rx).await;
    assert_eq!(
        storage::load_agent(&agent.id)
            .unwrap()
            .permission_mode
            .as_deref(),
        Some("bypassPermissions"),
        "the new choice must be persisted, or the next Turn reverts"
    );

    let args = std::fs::read_to_string(&args_log).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 2, "one `claude` per Turn: {args}");
    assert!(lines[0].contains("--permission-mode plan"), "{}", lines[0]);
    assert!(
        lines[1].contains("--permission-mode bypassPermissions"),
        "{}",
        lines[1]
    );
}

/// No pick means the Agent acts freely inside its Worktree — the Mode every
/// Agent ran in before the picker existed.
#[tokio::test]
async fn no_picked_mode_bypasses_permissions() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

    let agent = rt
        .spawn(
            &project,
            "hello".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.permission_mode, None);
    wait_for_exit(&mut rx).await;

    let args = std::fs::read_to_string(&args_log).unwrap();
    assert!(
        args.contains("--permission-mode bypassPermissions"),
        "{args}"
    );
}

/// Attachments reach `claude` as paths it is told about and allowed to read,
/// and reach the transcript beside the prompt rather than inside it.
/// Options are the Agent's, not a Turn's: set at Spawn or later, they reach
/// every Turn after, where the same slash command would last one.
#[tokio::test]
async fn options_reach_every_turn_as_settings() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

    let agent = rt
        .spawn(
            &project,
            "first".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;

    rt.edit(&agent.id, |a| {
        a.options = AgentOptions {
            advisor: Some("opus".into()),
            output_style: None,
        }
    })
    .await
    .unwrap();
    // Its own announcement, which reads as an idle Agent: out of the way of
    // the next wait for a Turn to end.
    rx.recv().await.unwrap();
    rt.resume(&agent.id, "second".into(), vec![], None, None, None)
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;
    rt.resume(&agent.id, "third".into(), vec![], None, None, None)
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;

    let args = std::fs::read_to_string(&args_log).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 3, "one `claude` per Turn: {args}");
    assert!(
        !lines[0].contains("--settings"),
        "nothing set, nothing passed: {}",
        lines[0]
    );
    for line in &lines[1..] {
        assert!(
            line.contains(r#"--settings {"advisorModel":"opus"}"#),
            "{line}"
        );
    }
}

/// The supervisor saves its own copy of the record when the Turn ends. A
/// rename, colour or option set while the Agent worked must outlive that save.
#[tokio::test]
async fn an_edit_made_while_the_agent_works_survives_the_turn() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let bin = write_script(
        r#"#!/bin/sh
sleep 1
echo '{"type":"result","status":"complete"}'
exit 0
"#,
    );
    let (rt, mut rx) = AgentRuntime::with_bin(bin);

    let agent = rt
        .spawn(
            &project,
            "slow".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let edited = rt
        .edit(&agent.id, |a| {
            a.user_title = Some("Mine".into());
            a.color = Some("green".into());
            a.options.output_style = Some("Concise".into());
        })
        .await
        .unwrap();
    assert_eq!(
        edited.state,
        AgentState::Running,
        "editing doesn't touch the Turn"
    );

    let done = wait_for_exit(&mut rx).await;
    assert_eq!(done.state, AgentState::Completed);
    for a in [done, storage::load_agent(&agent.id).unwrap()] {
        assert_eq!(a.user_title.as_deref(), Some("Mine"));
        assert_eq!(a.color.as_deref(), Some("green"));
        assert_eq!(a.options.output_style.as_deref(), Some("Concise"));
    }
}

#[tokio::test]
async fn attachments_are_named_to_claude_and_kept_out_of_the_prompt() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let files = tempfile::tempdir().unwrap();
    let shot = files.path().join("shot.png");
    std::fs::write(&shot, b"png").unwrap();
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));

    let agent = rt
        .spawn(
            &project,
            "fix this".into(),
            vec![shot.clone()],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.task.prompt, "fix this");
    assert_eq!(agent.task.attachments, vec![shot.clone()]);
    wait_for_exit(&mut rx).await;

    let args = std::fs::read_to_string(&args_log).unwrap();
    assert!(args.contains(&format!("- {}", shot.display())), "{args}");
    assert!(
        args.contains(&format!("--add-dir {}", files.path().display())),
        "{args}"
    );

    let logged = storage::read_events(&agent.id).unwrap();
    let prompt = logged
        .iter()
        .find(|e| e.event["type"] == PROMPT_EVENT_TYPE)
        .unwrap();
    assert_eq!(prompt.event["prompt"], "fix this");
    assert_eq!(
        prompt.event["attachments"],
        serde_json::json!([shot.display().to_string()])
    );

    let err = rt
        .resume(
            &agent.id,
            "and this".into(),
            vec![files.path().join("gone.png")],
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AttachmentMissing { .. }), "{err:?}");
}

#[tokio::test]
async fn resume_is_refused_while_the_agent_is_working() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let (rt, _rx) = AgentRuntime::with_bin(fake_claude_hang());

    let agent = rt
        .spawn(
            &project,
            "hang".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let err = rt
        .resume(&agent.id, "hurry up".into(), vec![], None, None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AgentBusy(_)), "{err:?}");

    rt.stop(&agent.id).await.unwrap();
}

/// A fake `claude` that plays both roles: a Turn when invoked like one, and
/// the namer when it sees the namer's flags. Every naming bumps a counter
/// file and answers with it, so a test can see how many times it was asked.
fn fake_claude_and_namer(counter: &std::path::Path) -> String {
    write_script(&format!(
        r#"#!/bin/sh
for a in "$@"; do
  if [ "$a" = "--safe-mode" ]; then
    n=$(cat {counter} 2>/dev/null || echo 0)
    n=$((n+1))
    echo $n > {counter}
    echo "Name $n"
    exit 0
  fi
done
echo '{{"type":"system","event":"init"}}'
echo '{{"type":"result","result":"did the thing"}}'
exit 0
"#,
        counter = counter.display()
    ))
}

/// Wait for the Agent record to come back carrying a name we have not seen.
async fn wait_for_name(
    rx: &mut mpsc::UnboundedReceiver<RuntimeEvent>,
    previous: Option<&str>,
) -> Agent {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("the namer should answer")
            .expect("channel open");
        if let RuntimeEvent::StateChanged { agent, .. } = ev {
            if agent.title.is_some() && agent.title.as_deref() != previous {
                return *agent;
            }
        }
    }
}

#[tokio::test]
async fn a_finished_turn_names_the_agent() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let counter = std::env::temp_dir().join(format!("cw-namer-{}", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin_naming(fake_claude_and_namer(&counter));

    let agent = rt
        .spawn(
            &project,
            "do the thing".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(agent.title, None, "a fresh Agent has no name to show yet");

    let named = wait_for_name(&mut rx, None).await;
    assert_eq!(named.title.as_deref(), Some("Name 1"));
    assert_eq!(
        storage::load_agent(&agent.id).unwrap().title.as_deref(),
        Some("Name 1"),
        "the name is persisted, not just announced"
    );
}

/// The name settles: rewritten at the end of Turns 1-3, untouched after.
#[tokio::test]
async fn the_name_stops_changing_after_the_third_turn() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let counter = std::env::temp_dir().join(format!("cw-namer-{}", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin_naming(fake_claude_and_namer(&counter));

    let agent = rt
        .spawn(
            &project,
            "first".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    let mut name = wait_for_name(&mut rx, None).await.title;
    assert_eq!(name.as_deref(), Some("Name 1"));

    for turn in 2..=3 {
        rt.resume(&agent.id, format!("turn {turn}"), vec![], None, None, None)
            .await
            .unwrap();
        name = wait_for_name(&mut rx, name.as_deref()).await.title;
        assert_eq!(name.as_deref(), Some(&*format!("Name {turn}")));
    }

    // The fourth Turn runs, but nothing asks for a name.
    rt.resume(&agent.id, "turn 4".into(), vec![], None, None, None)
        .await
        .unwrap();
    while !rt.running().await.is_empty() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Long enough that a namer, had one been spawned, would have answered:
    // the fake is a shell script, and the three before it took milliseconds.
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert_eq!(
        std::fs::read_to_string(&counter).unwrap().trim(),
        "3",
        "the namer runs for the first three Turns only"
    );
    let settled = storage::load_agent(&agent.id).unwrap();
    assert_eq!(settled.turns, 4);
    assert_eq!(settled.title.as_deref(), Some("Name 3"));
}

#[tokio::test]
async fn a_stopped_agent_can_still_be_resumed() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let args_log = std::env::temp_dir().join(format!("cw-args-{}.txt", new_id()));
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_hang());

    let agent = rt
        .spawn(
            &project,
            "go off the rails".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    rt.stop(&agent.id).await.unwrap();
    let stopped = wait_for_exit(&mut rx).await;
    assert_eq!(stopped.state, AgentState::Stopped);

    // Redirecting a Stopped Agent is the point of keeping its worktree.
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_recording(&args_log));
    let resumed = rt
        .resume(
            &agent.id,
            "do it differently".into(),
            vec![],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(resumed.turns, 2);
    assert_eq!(wait_for_exit(&mut rx).await.state, AgentState::Completed);
}

#[tokio::test]
async fn resume_is_refused_when_the_worktree_is_gone() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_ok());

    let agent = rt
        .spawn(
            &project,
            "hello".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
    wait_for_exit(&mut rx).await;
    worktree::discard(&project.path, &agent.worktree_path, &agent.branch)
        .await
        .unwrap();

    let err = rt
        .resume(&agent.id, "more".into(), vec![], None, None, None)
        .await
        .unwrap_err();
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
        task: Task {
            prompt: "from before sessions existed".into(),
            attachments: vec![],
        },
        state: AgentState::Completed,
        worktree_path: std::env::temp_dir(),
        base_commit: None,
        session_id: None,
        model: None,
        effort: None,
        permission_mode: None,
        options: Default::default(),
        turns: 1,
        title: None,
        user_title: None,
        color: None,
        branch: "cw/agent-old".into(),
        spawned_at: OffsetDateTime::now_utc(),
        turn_started_at: Some(OffsetDateTime::now_utc()),
        exited_at: Some(OffsetDateTime::now_utc()),
        exit_code: Some(0),
        fail_reason: None,
        merged_branch: None,
        merged_at: None,
        resolves: None,
    };
    storage::save_agent(&a).unwrap();
    let (rt, _rx) = AgentRuntime::with_bin(fake_claude_ok());

    let err = rt
        .resume(&a.id, "carry on".into(), vec![], None, None, None)
        .await
        .unwrap_err();
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

    let agent = rt
        .spawn(
            &project,
            "hello".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();
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
    // Seed a Running agent on disk (as if it were running when its Host died).
    let a = Agent {
        id: new_id(),
        project_id: new_id(),
        task: Task {
            prompt: "x".into(),
            attachments: vec![],
        },
        state: AgentState::Running,
        worktree_path: "/tmp/wt".into(),
        base_commit: None,
        session_id: Some("11111111-2222-4333-8444-555555555555".into()),
        model: None,
        effort: None,
        permission_mode: None,
        options: Default::default(),
        turns: 1,
        title: None,
        user_title: None,
        color: None,
        branch: "cw/agent-x".into(),
        spawned_at: OffsetDateTime::now_utc(),
        turn_started_at: Some(OffsetDateTime::now_utc()),
        exited_at: None,
        exit_code: None,
        fail_reason: None,
        merged_branch: None,
        merged_at: None,
        resolves: None,
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

async fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A registered Project whose one Agent has committed a change to
/// `shared.txt` that collides with one the user made on `main` since, so
/// merging it conflicts.
async fn conflicted_agent() -> (TempDir, Project, Agent) {
    let repo = init_repo().await;
    let project = sample_project(repo.path().to_path_buf());
    storage::Registry {
        projects: vec![project.clone()],
    }
    .save()
    .unwrap();

    std::fs::write(repo.path().join("shared.txt"), "original\n").unwrap();
    git(repo.path(), &["add", "-A"]).await;
    git(repo.path(), &["commit", "-qm", "add shared"]).await;

    let (rt, mut rx) = AgentRuntime::with_bin(write_script(
        r#"#!/bin/sh
echo 'the agent' > shared.txt
git commit -qam 'agent: edit shared'
exit 0
"#,
    ));
    rt.spawn(
        &project,
        "edit shared".into(),
        vec![],
        None,
        None,
        None,
        AgentOptions::default(),
    )
    .await
    .unwrap();
    let agent = wait_for_exit(&mut rx).await;
    assert_eq!(agent.state, AgentState::Completed);

    std::fs::write(repo.path().join("shared.txt"), "the user\n").unwrap();
    git(repo.path(), &["commit", "-qam", "user: edit shared"]).await;

    let conflict = merging::merge(&project.path, &mut agent.clone(), "main")
        .await
        .unwrap_err();
    assert!(
        matches!(conflict, Error::MergeConflict { .. }),
        "{conflict:?}"
    );
    (repo, project, agent)
}

/// The Agent that resolves the conflict, and the one that had it, both end
/// up Merged — which is what files both under Delivered.
#[tokio::test]
async fn a_resolver_that_resolves_merges_both_agents() {
    let _env = StateEnv::new();
    let (repo, project, conflicted) = conflicted_agent().await;

    let (rt, mut rx) = AgentRuntime::with_bin(write_script(
        r#"#!/bin/sh
git merge main >/dev/null 2>&1
printf 'the agent\nthe user\n' > shared.txt
git add -A
git commit -q --no-edit
exit 0
"#,
    ));
    let files = vec!["shared.txt".to_string()];
    let resolver = rt
        .spawn_resolver(&project, &conflicted, "main", &files, None, None)
        .await
        .unwrap();
    assert_eq!(
        resolver.resolves,
        Some(Resolution {
            agent_id: conflicted.id.clone(),
            target: "main".into()
        })
    );
    assert_eq!(
        resolver.permission_mode.as_deref(),
        Some(crate::domain::DEFAULT_PERMISSION_MODE),
        "a resolver must be free to write"
    );
    assert!(resolver.task.prompt.contains("`shared.txt`"));

    let done = wait_for_exit(&mut rx).await;
    assert_eq!(done.state, AgentState::Completed);
    assert_eq!(done.merged_branch.as_deref(), Some("main"));
    assert!(done.merged_at.is_some());

    // The conflicted Agent is announced with its new record too.
    let announced = loop {
        match rx.recv().await.expect("channel open") {
            RuntimeEvent::StateChanged { agent, .. } if agent.id == conflicted.id => break *agent,
            _ => continue,
        }
    };
    assert_eq!(announced.merged_branch.as_deref(), Some("main"));
    let original = storage::load_agent(&conflicted.id).unwrap();
    assert_eq!(original.merged_branch.as_deref(), Some("main"));

    // Main has the resolution and both branches, and neither Agent holds
    // anything the Project hasn't got.
    assert_eq!(
        std::fs::read_to_string(repo.path().join("shared.txt")).unwrap(),
        "the agent\nthe user\n"
    );
    for branch in [&conflicted.branch, &resolver.branch] {
        assert!(git::is_ancestor(repo.path(), branch, "main").await.unwrap());
    }
    assert!(!git::holds_unmerged_work(&original.worktree_path).await);
    assert!(!git::holds_unmerged_work(&done.worktree_path).await);

    let log = storage::read_events(&resolver.id).unwrap();
    assert!(
        log.iter().any(|e| e.event["type"] == NOTICE_EVENT_TYPE
            && e.event["text"]
                .as_str()
                .unwrap()
                .starts_with("Conflicts resolved")),
        "{log:?}"
    );
}

/// A Resolver that finishes without having merged the target in leaves the
/// Project alone and says why, rather than merging a branch that would just
/// conflict again.
#[tokio::test]
async fn a_resolver_that_did_not_merge_leaves_the_project_alone() {
    let _env = StateEnv::new();
    let (repo, project, conflicted) = conflicted_agent().await;
    let before = git(repo.path(), &["rev-parse", "main"]).await;

    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_ok());
    let files = vec!["shared.txt".to_string()];
    let resolver = rt
        .spawn_resolver(&project, &conflicted, "main", &files, None, None)
        .await
        .unwrap();
    let done = wait_for_exit(&mut rx).await;

    assert_eq!(done.state, AgentState::Completed);
    assert_eq!(done.merged_at, None);
    assert_eq!(storage::load_agent(&conflicted.id).unwrap().merged_at, None);
    assert_eq!(git(repo.path(), &["rev-parse", "main"]).await, before);

    let log = storage::read_events(&resolver.id).unwrap();
    assert!(
        log.iter().any(|e| e.event["type"] == NOTICE_EVENT_TYPE
            && e.event["text"].as_str().unwrap().starts_with("Not merged")),
        "{log:?}"
    );
}

#[tokio::test]
async fn a_host_closes_only_when_idle_and_then_takes_no_turns() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let (rt, mut rx) = AgentRuntime::with_bin(fake_claude_hang());
    let project = sample_project(repo.path().to_path_buf());
    let agent = rt
        .spawn(
            &project,
            "go".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await
        .unwrap();

    assert!(!rt.close_if_idle().await, "closed with a Turn running");
    rt.stop(&agent.id).await.unwrap();
    wait_for_exit(&mut rx).await;
    assert!(rt.close_if_idle().await);

    let spawned = rt
        .spawn(
            &project,
            "again".into(),
            vec![],
            None,
            None,
            None,
            AgentOptions::default(),
        )
        .await;
    assert!(matches!(spawned, Err(Error::HostClosing)));
    let resumed = rt
        .resume(&agent.id, "more".into(), vec![], None, None, None)
        .await;
    assert!(matches!(resumed, Err(Error::HostClosing)));
}
