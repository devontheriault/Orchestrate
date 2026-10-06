//! This state directory's Host, as a tool asks it something. Every call opens
//! a connection of its own and closes it once answered. So the server holds
//! nothing between calls, and a Host that restarts to update is simply there
//! again for the next one.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::host::local;
use crate::host::protocol::{Frame, Request, PROTOCOL};
use crate::paths;

/// How long a Host gets to answer one call. Longer than any read takes, but a
/// Host that hangs shouldn't hang the Agent asking it with it.
const PATIENCE: Duration = Duration::from_secs(60);

/// The Host on this machine, for this state directory: the one whose socket
/// sits in it. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Host {
    socket: PathBuf,
    /// What its machine calls itself, from its Hello.
    name: Arc<str>,
    /// The Agent this server serves, when it is a Turn's (see
    /// [`super::AGENT_ENV`]).
    agent: Option<Arc<str>>,
}

impl Host {
    /// The Host for this state directory, or why it can't be reached.
    pub async fn local() -> Result<Self, String> {
        let socket = paths::host_socket().map_err(|e| e.to_string())?;
        let agent = std::env::var(super::AGENT_ENV)
            .ok()
            .filter(|id| !id.is_empty());
        Self::at(socket, agent).await
    }

    /// The Host listening on `socket`, checked by asking it once.
    async fn at(socket: PathBuf, agent: Option<String>) -> Result<Self, String> {
        let mut host = Self {
            socket,
            name: Arc::from(""),
            agent: agent.map(Arc::from),
        };
        let hello = host.ask(None).await?;
        host.name = Arc::from(hello.as_str().unwrap_or_default());
        Ok(host)
    }

    /// What the Host's machine calls itself, as a window labels its Agents.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The id of the Agent whose Turn this server serves, or why there is
    /// none, for a tool that acts as that Agent.
    pub fn agent(&self) -> Result<&str, String> {
        self.agent.as_deref().ok_or_else(|| {
            "only an Orchestrate Agent can do this, from one of its own Turns; this MCP \
             client isn't one"
                .into()
        })
    }

    /// Ask the Host what a window would, as `{"method": method, "args": args}`
    /// (see `host/calls.rs`), and get its answer or its readable refusal.
    pub async fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        self.ask(Some((method, args))).await
    }

    /// As [`call`](Self::call), with the answer read as a `T`.
    pub async fn call_as<T: DeserializeOwned>(
        &self,
        method: &str,
        args: Value,
    ) -> Result<T, String> {
        let answer = self.call(method, args).await?;
        serde_json::from_value(answer)
            .map_err(|e| format!("the Host's answer to {method} didn't read as expected: {e}"))
    }

    /// One connection: the Host's Hello, then `call` and its reply. With no
    /// call, the Host's name from its Hello.
    async fn ask(&self, call: Option<(&str, Value)>) -> Result<Value, String> {
        tokio::time::timeout(PATIENCE, self.exchange(call))
            .await
            .unwrap_or_else(|_| Err("the Host didn't answer in time".into()))
    }

    async fn exchange(&self, call: Option<(&str, Value)>) -> Result<Value, String> {
        let stream = local::connect(&self.socket)
            .await
            .map_err(|_| self.not_running())?;
        let (read, mut write) = tokio::io::split(stream);
        let mut lines = BufReader::new(read).lines();

        let hello = match next(&mut lines).await? {
            Frame::Hello(hello) => hello,
            Frame::Refused(why) => return Err(format!("the Host refused the connection: {why}")),
            _ => return Err("the Host didn't introduce itself".into()),
        };
        if hello.protocol != PROTOCOL {
            return Err(format!(
                "the running Host is Orchestrate {} and this is {}, which can't talk to \
                 each other. Restart the Orchestrate app so both are the same version.",
                hello.version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        let Some((method, args)) = call else {
            return Ok(Value::String(hello.name));
        };

        const ID: u64 = 1;
        let mut request = serde_json::to_string(&Request {
            id: ID,
            call: json!({ "method": method, "args": args }),
        })
        .expect("requests always serialize");
        request.push('\n');
        write
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("lost the connection to the Host: {e}"))?;

        // Every connection hears every Agent's events; only the reply matters.
        loop {
            if let Frame::Reply { id: ID, outcome } = next(&mut lines).await? {
                return outcome.into();
            }
        }
    }

    fn not_running(&self) -> String {
        // The state directory rather than the socket, which on Windows is a
        // pipe name nobody would recognise.
        let dir = paths::state_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        format!(
            "no Orchestrate Host is running for {dir}. Open the Orchestrate app, which \
             starts one, and try again."
        )
    }
}

/// The Host's next frame, skipping any line this build can't read.
async fn next<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<Frame, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        match lines.next_line().await {
            Ok(Some(l)) => {
                if let Ok(frame) = serde_json::from_str(&l) {
                    return Ok(frame);
                }
            }
            Ok(None) => return Err("the Host hung up".into()),
            Err(e) => return Err(format!("lost the connection to the Host: {e}")),
        }
    }
}
