//! `orchestrate --mcp`: the app's MCP server, over stdio (ADR 0017). It is how
//! an Agent reaches the user's Spaces, and how any other MCP client does too,
//! such as Claude Desktop or a plain `claude`. Every Turn is handed it through
//! `--mcp-config` (see [`turn_args`]), so Agents need no setup. Anything
//! else adds the same command: `claude mcp add orchestrate -- orchestrate --mcp`.
//!
//! It only translates. It connects to this state directory's Host (see
//! [`Host`]) and turns each tool call into ordinary Host calls, the same ones
//! `host/calls.rs` answers for a window. It keeps no state and no logic of its
//! own, so a tool can do nothing a window can't. With no Host running it
//! exits, and says why.
//!
//! # Adding a tool
//!
//! A Space keeps its tools beside its own code, in `<space>/mcp.rs`, and
//! nothing about any one Space lives in this module. The Agents' tools in
//! `runtime/mcp.rs` are the example to copy.
//!
//! 1. Make sure the Host can answer what the tool needs, with a call in
//!    `host/calls.rs`. Reuse the window's call where there is one. The tool
//!    only translates, and the work is the Host's.
//! 2. In `<space>/mcp.rs`, write the tool as an async function taking the
//!    [`Host`] and its arguments. The arguments arrive as one struct deriving
//!    `Deserialize` and `schemars::JsonSchema`, and the struct's doc comments
//!    are what the model reads about each one, so write them for the model. A
//!    tool with no arguments takes [`NoArgs`]. Call the Host with
//!    [`Host::call`] or [`Host::call_as`]. Return `Ok` with anything
//!    `Serialize` for the model to read, which goes as JSON in the order its
//!    fields are written, or as plain text if it's a string. Or return `Err`
//!    with a sentence saying what went wrong. An `Err` reaches the model as a
//!    failed tool call, not a broken server.
//! 3. List the tool in the Space's `pub fn tools() -> Vec<Tool>`. Make it with
//!    [`Tool::reads`] if it only reads, or [`Tool::writes`] if it changes
//!    something on this machine. The difference matters: every Turn may call
//!    a reading tool, but a writing one only runs in a Turn that may write,
//!    never in plan mode (see [`turn_args`]).
//! 4. Add the Space's `tools` to [`SPACES`], once per Space.
//!
//! Name a tool for what it does, in snake_case and with the Space's noun in it
//! (`search_notes`, `list_agents`). A client lists every server's tools side by
//! side, and Claude Code shows them as `mcp__orchestrate__<name>`. The
//! description is the model's only guide to when to use the tool: say what it
//! returns and when it helps, in a sentence or two.
//!
//! # What must never be a tool
//!
//! **Nothing that leaves the machine.** Sending an email, inviting someone to
//! a Calendar event, answering an invitation, pushing a branch: those are the
//! user's to do, from the Space, just as only the user Merges or Pushes (ADR
//! 0014). A tool may *draft* any of them, since a draft stays here for the user
//! to send or throw away. It must never send one, and must never take a flag
//! that would. Reading is free. Writing something local that the user can see
//! and undo, such as a note, is allowed. Every tool is marked closed-world to
//! the client, which says the same.
//!
//! **Nothing the user couldn't undo.** No tool deletes a note: throwing one
//! away is the user's, from the Space.
//!
//! **No Merge.** An Agent may lead Helpers and merge their branches into its
//! own with git (ADR 0019), but bringing work into the Project's branches is
//! only ever the user's.
//!
//! # Who is asking
//!
//! A Turn's server is told which Agent it serves, in [`AGENT_ENV`]. A tool
//! that acts for that Agent, as the Lead tools do, reads it with
//! [`Host::agent`] and hands it to the Host, which decides what that Agent
//! may do. Any other client has no Agent, and those tools refuse it.

mod link;

#[cfg(test)]
mod tests;

pub use link::Host;

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::handler::server::common::schema_for_input;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::paths;

/// What the server is called, in a client's list and in every tool's name.
pub const SERVER_NAME: &str = "orchestrate";

/// The variable a Turn's server finds its Agent's id in (see [`turn_config`]).
pub const AGENT_ENV: &str = "ORCHESTRATE_AGENT";

/// Every Space's tools, one entry per Space.
const SPACES: &[fn() -> Vec<Tool>] = &[
    crate::runtime::mcp::tools,
    crate::notes::mcp::tools,
    crate::calendar::mcp::tools,
];

/// Every tool the server offers.
pub fn tools() -> Vec<Tool> {
    SPACES.iter().flat_map(|space| space()).collect()
}

/// The arguments of a tool that takes none.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoArgs {}

/// A tool's answer, as the text the model reads.
type Answer = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;

/// One tool a Space offers. See the module docs for how to add one.
pub struct Tool {
    name: &'static str,
    description: &'static str,
    input: Arc<JsonObject>,
    writes: bool,
    run: Arc<dyn Fn(Host, Value) -> Answer + Send + Sync>,
}

impl Tool {
    /// A tool that only reads. `run` gets the Host and the call's arguments.
    pub fn reads<A, R, F, Fut>(name: &'static str, description: &'static str, run: F) -> Self
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(Host, A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R, String>> + Send + 'static,
        R: Serialize,
    {
        Self::new(name, description, false, run)
    }

    /// A tool that changes something on this machine, which the user can see
    /// and undo. Never one that sends anything off it (see the module docs).
    pub fn writes<A, R, F, Fut>(name: &'static str, description: &'static str, run: F) -> Self
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(Host, A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R, String>> + Send + 'static,
        R: Serialize,
    {
        Self::new(name, description, true, run)
    }

    fn new<A, R, F, Fut>(
        name: &'static str,
        description: &'static str,
        writes: bool,
        run: F,
    ) -> Self
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: Fn(Host, A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R, String>> + Send + 'static,
        R: Serialize,
    {
        // Fails only for arguments that aren't a struct: a mistake in the
        // tool, which the tests' listing of every tool catches.
        let input = schema_for_input::<A>().unwrap_or_else(|e| panic!("tool {name}: {e}"));
        let run = move |host: Host, args: Value| -> Answer {
            match serde_json::from_value::<A>(args) {
                Ok(args) => {
                    let answer = run(host, args);
                    Box::pin(async move { answer.await.map(|a| text(&a)) })
                }
                Err(e) => Box::pin(std::future::ready(Err(format!(
                    "{name} can't read its arguments: {e}"
                )))),
            }
        };
        Self {
            name,
            description,
            input,
            writes,
            run: Arc::new(run),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The tool as a client is told about it.
    fn listing(&self) -> rmcp::model::Tool {
        let hints = ToolAnnotations::new()
            .read_only(!self.writes)
            .open_world(false);
        let hints = if self.writes {
            hints
        } else {
            hints.destructive(false).idempotent(true)
        };
        rmcp::model::Tool::new(self.name, self.description, self.input.clone()).annotate(hints)
    }
}

/// A tool's answer as the model reads it: a string as it is, anything else as
/// JSON, its fields in the order they were written.
fn text<R: Serialize>(answer: &R) -> String {
    match serde_json::to_value(answer) {
        Ok(Value::String(s)) => s,
        _ => serde_json::to_string_pretty(answer).unwrap_or_default(),
    }
}

/// What every client is told about the server as a whole.
const INSTRUCTIONS: &str = "Orchestrate is the app the user runs Claude Code Agents in, \
beside their Notes, Calendar and Mail. These tools read it, on the user's own machine, and \
may write the user's notes, which are Markdown files they can see and undo. An Agent may also \
spawn helper Agents to work on parts of its task in parallel, each in its own Git worktree. \
Nothing here sends anything anywhere: what leaves the machine, the user sends from the app.";

struct Server {
    host: Host,
    tools: Vec<Tool>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            self.tools.iter().map(Tool::listing).collect(),
        ))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        self.tools
            .iter()
            .find(|t| t.name == name)
            .map(Tool::listing)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(tool) = self.tools.iter().find(|t| t.name == request.name) else {
            return Err(ErrorData::invalid_params(
                format!("there is no tool called {}", request.name),
                None,
            ));
        };
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = match (tool.run)(self.host.clone(), args).await {
            Ok(answer) => CallToolResult::success(vec![ContentBlock::text(answer)]),
            Err(why) => CallToolResult::error(vec![ContentBlock::text(why)]),
        };
        Ok(result.into())
    }
}

/// `--mcp`: serve MCP on stdin and stdout until the client hangs up. Nothing
/// else may write to stdout here, since that is the protocol's channel.
pub fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("could not start the async runtime");
    let code = runtime.block_on(main());
    std::process::exit(code);
}

async fn main() -> i32 {
    match serve().await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("orchestrate --mcp: {e}");
            1
        }
    }
}

async fn serve() -> Result<(), String> {
    let host = Host::local().await?;
    let server = Server {
        host,
        tools: tools(),
    }
    .serve(rmcp::transport::stdio())
    .await
    .map_err(|e| format!("could not start talking MCP: {e}"))?;
    server
        .waiting()
        .await
        .map_err(|e| format!("stopped talking MCP: {e}"))?;
    Ok(())
}

/// What every Turn of `agent` passes `claude` so it can use the server, as
/// arguments that each end before a flag: both options are variadic.
///
/// `--mcp-config` hands it the server (see [`turn_config`]). Claude Code adds
/// it to the user's and the Project's own MCP servers rather than replacing
/// them, which only `--strict-mcp-config` would do, and only a Mail-locked
/// Agent's Turns pass that (ADR 0018).
///
/// `--allowedTools` lets it call the tools that only read without asking,
/// which `--print` has no way to do. Without it a plan-mode Turn is refused
/// every tool. The tools that write are left out, so they run only where the
/// Agent may write anyway (YOLO), never in plan mode.
///
/// Empty when the server can't be described. The Turn then runs without it
/// rather than not at all.
pub fn turn_args(agent: &str) -> Vec<String> {
    let Some(config) = turn_config(agent) else {
        return vec![];
    };
    vec![
        "--mcp-config".into(),
        config,
        "--allowedTools".into(),
        turn_names(false).join(","),
    ]
}

/// The server's tools that write, as a Turn names them. A Mail-locked Agent's
/// Turns deny them outright (ADR 0018), so they aren't even offered, rather
/// than resting on `plan` to refuse them.
pub fn writing_tools() -> Vec<String> {
    turn_names(true)
}

/// The server's tools that write, or that only read, as a Turn names them.
fn turn_names(writes: bool) -> Vec<String> {
    tools()
        .iter()
        .filter(|t| t.writes == writes)
        .map(|t| turn_name(t.name))
        .collect()
}

/// One of the server's tools as a Turn names it, `mcp__orchestrate__<tool>`.
pub fn turn_name(tool: &str) -> String {
    format!("mcp__{SERVER_NAME}__{tool}")
}

/// The server as `claude --mcp-config` takes it for a Turn of `agent`: run
/// from this very binary against this state directory, so an Agent reaches its
/// own Host and no other, and told which Agent it serves. `None` when the
/// binary or the directory can't be found.
///
/// It goes inline, as the Options go to `--settings`, so there is no file to
/// write or leave stale.
fn turn_config(agent: &str) -> Option<String> {
    let exe = this_binary()?;
    // Absolute, since the Turn runs in its Worktree, not where the Host started.
    let state = std::path::absolute(paths::state_dir().ok()?).ok()?;
    Some(
        json!({
            "mcpServers": {
                SERVER_NAME: {
                    "type": "stdio",
                    "command": exe,
                    "args": ["--mcp"],
                    "env": { "ORCHESTRATE_STATE_DIR": state, AGENT_ENV: agent },
                }
            }
        })
        .to_string(),
    )
}

/// The binary this process runs. Inside an AppImage that is the file in its
/// mount, which lasts as long as the Host does, and so outlasts every Turn.
fn this_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.exists() {
        return Some(exe);
    }
    // Linux names a binary replaced since it started "<path> (deleted)", as a
    // rebuild leaves a development Host. The new one at the same path is the
    // build a window would bring up next, and talks to this Host as well.
    let replaced = exe.to_str()?.strip_suffix(" (deleted)")?;
    Some(PathBuf::from(replaced)).filter(|p| p.exists())
}
