use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use tokio::process::Command;

use super::*;
use crate::host::{self, local};
use crate::runtime::AgentRuntime;
use crate::test_util::{init_repo, write_script, StateEnv};

/// Set on the child a test starts as `--mcp`, so it serves rather than tests.
const HELPER: &str = "ORCHESTRATE_TEST_MCP";

/// Long enough for any step here; a hang fails the test rather than the run.
const PATIENCE: Duration = Duration::from_secs(20);

/// Not a test: `orchestrate --mcp` itself, for the tests below to start as a
/// child the way an MCP client does. It returns at once unless started so.
#[test]
fn serve_as_mcp_child() {
    if std::env::var(HELPER).as_deref() == Ok("1") {
        super::run();
    }
}

/// This test binary started as `--mcp`, talking on its stdin and stdout, in
/// whatever state directory the test has set.
fn mcp_child() -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "mcp::tests::serve_as_mcp_child",
        "--nocapture",
        "--quiet",
    ])
    .args(["--test-threads", "1"])
    .env(HELPER, "1")
    .kill_on_drop(true);
    cmd
}

/// A Host serving this state directory's socket, as `--host` would but in
/// this process, its Turns run by `claude`.
fn host_with(claude: &str) -> tokio::task::JoinHandle<()> {
    let (rt, rx) = AgentRuntime::with_bin(claude);
    let host = host::Host::new(rt, rx, vec![]);
    let socket = paths::host_socket().unwrap();
    let listener = local::Listener::bind(&socket).unwrap();
    tokio::spawn(host::serve(host, listener, std::future::ready(None)))
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, args: Value) -> CallToolResult {
    let Value::Object(args) = args else {
        panic!("arguments are an object");
    };
    let params = CallToolRequestParams::new(tool.to_owned()).with_arguments(args);
    tokio::time::timeout(PATIENCE, client.call_tool(params))
        .await
        .expect("the server went quiet")
        .unwrap()
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_mcp_client_reads_the_hosts_agents_over_stdio() {
    let _env = StateEnv::new();
    paths::ensure_dirs().unwrap();
    let claude = write_script(
        r#"#!/bin/sh
echo '{"type":"result","subtype":"success","result":"Fixed the login bug and added a test."}'
exit 0
"#,
    );
    let serving = host_with(&claude);

    // Set up as a window would, through the Host.
    let host = Host::local().await.unwrap();
    let repo = init_repo().await;
    let project = host
        .call(
            "add_project",
            json!({ "name": "Shop", "path": repo.path(), "setUp": false }),
        )
        .await
        .unwrap();
    let agent = host
        .call(
            "spawn_agent",
            json!({
                "projectId": project["id"],
                "prompt": "Fix the login bug\nand add a test",
                "attachments": [],
                "model": null,
                "effort": null,
                "permissionMode": null,
                "options": {},
            }),
        )
        .await
        .unwrap();
    let id = agent["id"].as_str().unwrap().to_owned();
    let done = async {
        loop {
            let agents = host.call("list_agents", json!({})).await.unwrap();
            if agents[0]["state"] != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(PATIENCE, done)
        .await
        .expect("the Turn never ended");

    let client =
        ().serve(TokioChildProcess::new(mcp_child()).unwrap())
            .await
            .expect("the server should start");

    let listed: HashSet<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.into_owned())
        .collect();
    for tool in ["list_agents", "agent_status", "agent_last_answer"] {
        assert!(listed.contains(tool), "{tool} missing from {listed:?}");
    }

    let agents = call(&client, "list_agents", json!({})).await;
    assert_ne!(agents.is_error, Some(true), "{}", text(&agents));
    // In the order the tool wrote them, not alphabetical, so the id leads.
    assert!(
        text(&agents).starts_with("[\n  {\n    \"id\""),
        "{}",
        text(&agents)
    );
    let agents: Value = serde_json::from_str(&text(&agents)).unwrap();
    assert_eq!(
        agents,
        json!([{
            "id": id,
            "title": "Fix the login bug",
            "state": "completed",
            "project": "Shop",
            "host": host::protocol::machine_name(),
        }])
    );

    // By Title as well as by id.
    let status = call(
        &client,
        "agent_status",
        json!({ "agent": "fix the login bug" }),
    )
    .await;
    let status: Value = serde_json::from_str(&text(&status)).unwrap();
    assert_eq!(status["id"], id);
    assert_eq!(status["task"], "Fix the login bug\nand add a test");
    assert_eq!(status["turns"], 1);

    let answer = call(&client, "agent_last_answer", json!({ "agent": id })).await;
    assert_eq!(text(&answer), "Fixed the login bug and added a test.");

    let unknown = call(&client, "agent_status", json!({ "agent": "nobody" })).await;
    assert_eq!(unknown.is_error, Some(true));
    assert!(text(&unknown).contains("list_agents"), "{}", text(&unknown));

    client.cancel().await.unwrap();
    serving.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_host_running_it_exits_and_says_why() {
    let _env = StateEnv::new();
    let out = tokio::time::timeout(
        PATIENCE,
        mcp_child()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    .expect("it should give up at once")
    .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("no Orchestrate Host is running"), "{said}");
}

#[test]
fn every_tool_has_its_own_name_and_an_object_for_arguments() {
    let mut names = HashSet::new();
    for tool in tools() {
        assert!(
            names.insert(tool.name()),
            "two tools called {}",
            tool.name()
        );
        let listing = tool.listing();
        assert_eq!(listing.input_schema["type"], "object", "{}", tool.name());
        // Nothing may leave the machine; see the module docs.
        let hints = listing.annotations.unwrap();
        assert_eq!(hints.open_world_hint, Some(false), "{}", tool.name());
    }
}

/// A plan-mode Turn can't answer a permission prompt, so the reading tools
/// are allowed up front. The writing ones aren't: plan mode writes nothing.
#[test]
fn a_turn_may_call_the_reading_tools_without_asking() {
    let _env = StateEnv::new();
    let args = turn_args();
    let at = args.iter().position(|a| a == "--allowedTools").unwrap();
    let allowed: HashSet<&str> = args[at + 1].split(',').collect();
    let reading: HashSet<String> = tools()
        .iter()
        .filter(|t| !t.writes)
        .map(|t| format!("mcp__orchestrate__{}", t.name()))
        .collect();
    assert_eq!(allowed, reading.iter().map(String::as_str).collect());
    assert!(allowed.contains("mcp__orchestrate__list_agents"));
}

#[test]
fn a_turn_runs_this_binary_against_this_state_directory() {
    let _env = StateEnv::new();
    let config: Value = serde_json::from_str(&turn_config().unwrap()).unwrap();
    let server = &config["mcpServers"][SERVER_NAME];
    assert_eq!(server["command"], json!(std::env::current_exe().unwrap()));
    assert_eq!(server["args"], json!(["--mcp"]));
    assert_eq!(
        server["env"]["ORCHESTRATE_STATE_DIR"],
        json!(paths::state_dir().unwrap())
    );
}
