use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

use super::client::{HostLink, Status};
use super::protocol::{Frame, Hello, PROTOCOL};
use super::*;
use crate::storage;
use crate::test_util::{init_repo, write_script, StateEnv};

/// Long enough for any step here; a hang fails the test rather than the run.
const PATIENCE: Duration = Duration::from_secs(10);

fn fake_claude_ok() -> String {
    write_script(
        r#"#!/bin/sh
echo '{"type":"assistant","message":"hi"}'
echo '{"type":"result","status":"complete"}'
exit 0
"#,
    )
}

fn fake_claude_hang() -> String {
    write_script(
        r#"#!/bin/sh
echo '{"type":"system","event":"init"}'
while : ; do sleep 1; done
"#,
    )
}

/// A Host serving on `socket`, as `--host` would run it but in this process.
fn host_on(socket: &Path, claude: &str) -> (Arc<Host>, JoinHandle<()>) {
    let (rt, rx) = AgentRuntime::with_bin(claude);
    let host = Host::new(rt, rx, vec![]);
    let listener = UnixListener::bind(socket).unwrap();
    let serving = tokio::spawn(serve(host.clone(), listener));
    (host, serving)
}

/// A raw connection, speaking the protocol by hand.
struct Window {
    lines: Lines<BufReader<OwnedReadHalf>>,
    write: OwnedWriteHalf,
    next_id: u64,
    /// Events that arrived while waiting for a reply.
    events: VecDeque<(String, Value)>,
}

impl Window {
    async fn open(socket: &Path) -> (Self, Hello) {
        let (read, write) = UnixStream::connect(socket).await.unwrap().into_split();
        let mut window = Self {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 1,
            events: VecDeque::new(),
        };
        let Frame::Hello(hello) = window.frame().await else {
            panic!("the Host must open with its Hello");
        };
        (window, hello)
    }

    async fn frame(&mut self) -> Frame {
        let l = tokio::time::timeout(PATIENCE, self.lines.next_line())
            .await
            .expect("the Host went quiet")
            .unwrap()
            .expect("the Host hung up");
        serde_json::from_str(&l).unwrap()
    }

    async fn call(&mut self, method: &str, args: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "id": id, "call": { "method": method, "args": args } });
        self.write
            .write_all(format!("{req}\n").as_bytes())
            .await
            .unwrap();
        loop {
            match self.frame().await {
                Frame::Reply { id: got, outcome } if got == id => return outcome.into(),
                Frame::Event { name, payload } => self.events.push_back((name, payload)),
                _ => {}
            }
        }
    }

    async fn event(&mut self) -> (String, Value) {
        if let Some(e) = self.events.pop_front() {
            return e;
        }
        loop {
            if let Frame::Event { name, payload } = self.frame().await {
                return (name, payload);
            }
        }
    }

    /// Skip events until `agent_id` is reported in a state other than running.
    async fn exit_of(&mut self, agent_id: &str) -> Value {
        loop {
            let (name, payload) = self.event().await;
            if name == "agent-state-changed"
                && payload["id"] == agent_id
                && payload["state"] != "running"
            {
                return payload;
            }
        }
    }
}

async fn add_project(window: &mut Window, repo: &Path) -> String {
    let project = window
        .call(
            "add_project",
            json!({ "name": "T", "path": repo, "setUp": false }),
        )
        .await
        .unwrap();
    project["id"].as_str().unwrap().to_string()
}

async fn spawn(window: &mut Window, project_id: &str) -> String {
    let agent = window
        .call(
            "spawn_agent",
            json!({
                "projectId": project_id,
                "prompt": "do it",
                "attachments": [],
                "model": null,
                "effort": null,
                "permissionMode": null,
                "options": {},
            }),
        )
        .await
        .unwrap();
    agent["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_window_is_greeted_and_its_calls_answered() {
    let _env = StateEnv::new();
    let socket = paths::host_socket().unwrap();
    let (_host, _serving) = host_on(&socket, &fake_claude_ok());

    let (mut window, hello) = Window::open(&socket).await;
    assert_eq!(hello.protocol, PROTOCOL);

    assert_eq!(window.call("list_projects", json!({})).await, Ok(json!([])));
    let unknown = window.call("launch_rockets", json!({})).await;
    assert!(unknown.unwrap_err().contains("can't read this call"));
    // A bad call doesn't cost the window its connection.
    assert_eq!(window.call("list_agents", json!({})).await, Ok(json!([])));
}

#[tokio::test]
async fn every_window_hears_every_agent() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let socket = paths::host_socket().unwrap();
    let (_host, _serving) = host_on(&socket, &fake_claude_ok());

    let (mut spawner, _) = Window::open(&socket).await;
    let (mut watcher, _) = Window::open(&socket).await;
    let project = add_project(&mut spawner, repo.path()).await;
    let agent = spawn(&mut spawner, &project).await;

    for window in [&mut spawner, &mut watcher] {
        assert_eq!(window.exit_of(&agent).await["state"], "completed");
    }
}

#[tokio::test]
async fn closing_a_window_leaves_its_agents_working() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let socket = paths::host_socket().unwrap();
    let (_host, _serving) = host_on(&socket, &fake_claude_hang());

    let (mut first, _) = Window::open(&socket).await;
    let project = add_project(&mut first, repo.path()).await;
    let agent = spawn(&mut first, &project).await;
    drop(first);

    let (mut second, _) = Window::open(&socket).await;
    let agents = second.call("list_agents", json!({})).await.unwrap();
    assert_eq!(agents[0]["id"], agent);
    assert_eq!(agents[0]["state"], "running");
    second
        .call("stop_agent", json!({ "agentId": agent }))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_host_asked_to_make_way_waits_for_its_turns() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let socket = paths::host_socket().unwrap();
    let (_host, serving) = host_on(&socket, &fake_claude_hang());

    let (mut window, _) = Window::open(&socket).await;
    let project = add_project(&mut window, repo.path()).await;
    let agent = spawn(&mut window, &project).await;

    window.call("shutdown_when_idle", json!({})).await.unwrap();
    tokio::time::sleep(IDLE_POLL * 2).await;
    assert!(!serving.is_finished(), "exited with a Turn still running");

    window
        .call("stop_agent", json!({ "agentId": agent }))
        .await
        .unwrap();
    tokio::time::timeout(PATIENCE, serving)
        .await
        .expect("still serving once idle")
        .unwrap();
    // Stopped by the user, not Orphaned by the exit.
    assert_eq!(
        storage::load_agent(&agent).unwrap().state,
        crate::domain::AgentState::Stopped
    );
}

#[tokio::test]
async fn stopping_the_host_orphans_what_is_running() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let socket = paths::host_socket().unwrap();
    let (host, serving) = host_on(&socket, &fake_claude_hang());

    let (mut window, _) = Window::open(&socket).await;
    let project = add_project(&mut window, repo.path()).await;
    let agent = spawn(&mut window, &project).await;

    host.stop();
    tokio::time::timeout(PATIENCE, serving)
        .await
        .expect("still serving after stop")
        .unwrap();
    assert_eq!(
        storage::load_agent(&agent).unwrap().state,
        crate::domain::AgentState::Orphaned
    );
    // The next Host reports it, once.
    let adopted = runtime::adopt_orphans_on_launch().unwrap();
    assert_eq!(adopted.len(), 1);
    assert_eq!(adopted[0].id, agent);
    assert!(runtime::adopt_orphans_on_launch().unwrap().is_empty());
}

#[tokio::test]
async fn a_link_starts_a_host_and_follows_it_to_its_successor() {
    let _env = StateEnv::new();
    let socket = paths::host_socket().unwrap();
    let claude = fake_claude_ok();

    // Standing in for the service: each start brings up a fresh Host.
    let started = Arc::new(AtomicUsize::new(0));
    let hosts: Arc<Mutex<Vec<Arc<Host>>>> = Arc::default();
    let start = {
        let (socket, started, hosts) = (socket.clone(), started.clone(), hosts.clone());
        let tokio = tokio::runtime::Handle::current();
        Arc::new(move || {
            started.fetch_add(1, Ordering::SeqCst);
            let _entered = tokio.enter();
            let _ = std::fs::remove_file(&socket);
            let (host, _) = host_on(&socket, &claude);
            hosts.lock().unwrap().push(host);
            Ok(())
        })
    };
    let heard: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let notify = {
        let heard = heard.clone();
        Arc::new(move |name: &str, payload: Value| {
            heard.lock().unwrap().push((name.to_string(), payload));
        })
    };
    let link = HostLink::new(socket.clone(), start, notify);
    tokio::spawn(link.clone().run());

    assert_eq!(link.call("list_projects", json!({})).await, Ok(json!([])));
    assert_eq!(started.load(Ordering::SeqCst), 1);
    let Status::Connected {
        instance: first, ..
    } = link.status()
    else {
        panic!("not connected after a call went through");
    };

    // The Host goes away, as it does when it updates.
    hosts.lock().unwrap()[0].stop();
    let _ = std::fs::remove_file(&socket);

    let reconnected = tokio::time::timeout(PATIENCE, async {
        loop {
            if let Status::Connected { instance, .. } = link.status() {
                if instance != first {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(reconnected.is_ok(), "never reached the new Host");
    assert_eq!(started.load(Ordering::SeqCst), 2);
    assert_eq!(link.call("list_agents", json!({})).await, Ok(json!([])));

    // The webview heard it drop and come back.
    let states: Vec<String> = heard
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "host-status")
        .map(|(_, p)| p["state"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(states, ["connected", "connecting", "connected"]);
}

#[tokio::test]
async fn two_windows_sending_at_once_start_one_turn_and_queue_the_other() {
    let _env = StateEnv::new();
    let repo = init_repo().await;
    let socket = paths::host_socket().unwrap();
    let (_host, _serving) = host_on(&socket, &fake_claude_hang());

    let (mut laptop, _) = Window::open(&socket).await;
    let (mut desktop, _) = Window::open(&socket).await;
    let project = add_project(&mut laptop, repo.path()).await;
    let agent = spawn(&mut laptop, &project).await;
    laptop
        .call("stop_agent", json!({ "agentId": agent }))
        .await
        .unwrap();

    let say = |prompt: &str| {
        json!({
            "agentId": agent, "prompt": prompt, "attachments": [],
            "model": null, "effort": null, "permissionMode": null,
        })
    };
    let (a, b) = tokio::join!(
        laptop.call("send_message", say("from the laptop")),
        desktop.call("send_message", say("from the desktop")),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    let queued: Vec<usize> = [&a, &b]
        .iter()
        .map(|r| r["queue"].as_array().unwrap().len())
        .collect();
    assert!(queued.contains(&0) && queued.contains(&1), "{queued:?}");

    let now = laptop.call("list_agents", json!({})).await.unwrap();
    assert_eq!(now[0]["state"], "running");
    assert_eq!(now[0]["turns"], 2, "two Turns started in one Worktree");
    assert_eq!(now[0]["queue"].as_array().unwrap().len(), 1);
    laptop
        .call("stop_agent", json!({ "agentId": agent }))
        .await
        .unwrap();
}
