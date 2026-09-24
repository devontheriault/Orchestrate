//! Which slash commands `claude` offers in a directory, and which output styles.
//!
//! The composer's `/` menu is filled by asking `claude` itself rather than from
//! a list baked into the app. The same answer lists the output styles, which
//! the Agent options offer and which live in the same places as commands do. Built-in commands, the user's skills and plugins,
//! and the Project's own `.claude/commands` and `.claude/skills` all depend on
//! the Claude Code install and the directory, so only `claude` knows the list.
//!
//! We ask the way the Agent SDK does: start `claude` on a stream-json stdin and
//! send it the `initialize` control request. It answers with every command it
//! would accept, each with a description and argument hint, before any model
//! is called. So the question costs no tokens and leaves no session behind.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::error::{Error, Result};

/// Bounded so a `claude` that never answers leaves the menu showing an error
/// rather than "loading…" forever. It normally answers in under a second.
const TIMEOUT: Duration = Duration::from_secs(15);

const REQUEST_ID: &str = "cw-slash-commands";

/// One command the `/` menu offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommand {
    /// What follows the `/`, e.g. `compact` or `mattpocock-skills:tdd`.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// What the command takes after its name, e.g. `[interval] [prompt]`.
    /// Empty when `claude` doesn't say.
    #[serde(default, rename(deserialize = "argumentHint"))]
    pub argument_hint: String,
    /// Other names `claude` accepts for the same command, e.g. `tdd` for a
    /// plugin's namespaced `mattpocock-skills:tdd`.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// What `claude` offers a Turn started in one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Offered {
    pub commands: Vec<SlashCommand>,
    /// Output styles by name, `default` first — built-in, the user's and the
    /// Project's own.
    pub output_styles: Vec<String>,
}

/// Every slash command and output style `claude` would offer a Turn started
/// in `cwd`.
pub async fn list(bin: &str, cwd: &Path) -> Result<Offered> {
    let fail = |why: String| Error::SlashCommands(why);
    let mut child = Command::new(bin)
        .arg("--print")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| fail(format!("could not start `claude`: {e}")))?;

    // Closing stdin straight after the request is what lets `claude` exit once
    // it has answered: there will be no user message to wait for.
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let request = serde_json::json!({
        "type": "control_request",
        "request_id": REQUEST_ID,
        "request": { "subtype": "initialize" },
    });
    stdin
        .write_all(format!("{request}\n").as_bytes())
        .await
        .map_err(|e| fail(e.to_string()))?;
    drop(stdin);

    let stdout = child.stdout.take().expect("stdout was piped");
    let read = async {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(answer) = parse_answer(&line) {
                return answer;
            }
        }
        Err(fail("`claude` exited without listing its commands".into()))
    };
    // Dropping the child on the way out kills it, whatever it was doing.
    tokio::time::timeout(TIMEOUT, read)
        .await
        .map_err(|_| fail("`claude` took too long to answer".into()))?
}

/// What one line of `claude`'s output offers, if it is the answer to our
/// request. `None` for any other line.
fn parse_answer(line: &str) -> Option<Result<Offered>> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "control_response" {
        return None;
    }
    let response = v.get("response")?;
    if response.get("request_id")?.as_str()? != REQUEST_ID {
        return None;
    }
    if response.get("subtype").and_then(Value::as_str) != Some("success") {
        let why = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("`claude` refused to list its commands");
        return Some(Err(Error::SlashCommands(why.to_string())));
    }
    let answer = response.get("response");
    let field = |name: &str| {
        answer
            .and_then(|r| r.get(name))
            .cloned()
            .unwrap_or(Value::Array(vec![]))
    };
    let commands = match serde_json::from_value::<Vec<SlashCommand>>(field("commands")) {
        Ok(commands) => offered(commands),
        Err(e) => return Some(Err(Error::Json(e))),
    };
    // Only a nicety for `/output-style`, so a shape we don't recognise
    // leaves it empty rather than failing the commands too.
    let output_styles =
        serde_json::from_value(field("available_output_styles")).unwrap_or_default();
    Some(Ok(Offered {
        commands,
        output_styles,
    }))
}

/// Drop the commands `claude` lists for its own plumbing: `__`-prefixed names
/// are internal and never meant to be typed.
fn offered(commands: Vec<SlashCommand>) -> Vec<SlashCommand> {
    commands
        .into_iter()
        .filter(|c| !c.name.starts_with("__"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of a real answer: a plugin skill with an alias, a built-in
    /// with an argument hint, an internal command, and the output styles.
    const ANSWER: &str = r#"{"type":"control_response","response":{"subtype":"success","request_id":"cw-slash-commands","response":{"commands":[
        {"name":"mattpocock-skills:tdd","description":"(mattpocock-skills) Test-driven development.","argumentHint":"","aliases":["tdd"]},
        {"name":"compact","description":"Free up context by summarizing the conversation so far","argumentHint":"<optional custom summarization instructions>"},
        {"name":"__remote-workflow","description":"Run the workflow script","argumentHint":""}
    ],"available_output_styles":["default","Concise"],"models":[],"pid":1}}}"#;

    #[test]
    fn reads_commands_out_of_the_initialize_answer() {
        let offered = parse_answer(ANSWER).unwrap().unwrap();
        assert_eq!(offered.output_styles, vec!["default", "Concise"]);
        let commands = offered.commands;
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["mattpocock-skills:tdd", "compact"]);
        assert_eq!(commands[0].aliases, vec!["tdd"]);
        assert_eq!(
            commands[1].argument_hint,
            "<optional custom summarization instructions>"
        );
        assert!(commands[1].aliases.is_empty());
    }

    #[test]
    fn other_lines_are_not_the_answer() {
        assert!(parse_answer(r#"{"type":"system","subtype":"init"}"#).is_none());
        assert!(parse_answer("not json").is_none());
        let someone_elses = ANSWER.replace("cw-slash-commands", "other");
        assert!(parse_answer(&someone_elses).is_none());
    }

    #[test]
    fn a_refusal_is_an_error_carrying_its_reason() {
        let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"cw-slash-commands","error":"Already initialized"}}"#;
        let err = parse_answer(refused).unwrap().unwrap_err();
        assert!(err.to_string().contains("Already initialized"), "{err}");
    }

    /// The whole exchange against a stand-in `claude` that answers only once
    /// it has read the request, and only if the flags ask for stream-json.
    #[cfg(unix)]
    #[tokio::test]
    async fn asks_claude_over_stdin_and_reads_its_answer() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("claude");
        let script = format!(
            "#!/bin/sh\n\
             case \"$*\" in *'--input-format stream-json'*) ;; *) exit 2 ;; esac\n\
             read line\n\
             case \"$line\" in *initialize*) ;; *) exit 3 ;; esac\n\
             echo '{{\"type\":\"system\",\"subtype\":\"init\"}}'\n\
             cat <<'EOF'\n{}\nEOF\n",
            ANSWER.replace('\n', " ")
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let commands = list(bin.to_str().unwrap(), dir.path())
            .await
            .unwrap()
            .commands;
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[1].name, "compact");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_claude_that_never_answers_is_an_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = list("false", dir.path()).await.unwrap_err();
        assert!(err.to_string().contains("without listing"), "{err}");
    }
}
