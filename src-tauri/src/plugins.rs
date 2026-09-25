//! Claude Code's plugins on this Host: what's installed, what the marketplaces
//! offer, and the changes the `/plugin` window makes to them.
//!
//! In a terminal `/plugin` is an interactive screen, and `claude --print` has
//! no such command. But `claude plugin` does the same work from the command
//! line and answers in JSON, so the app asks it rather than reading or writing
//! Claude Code's plugin files itself. A plugin installed here is loaded by
//! every Turn after, since each Turn is a fresh `claude`.
//!
//! Some plugins install by running a command their marketplace declares.
//! Claude Code won't run one until a person has seen it, and neither does the
//! app: the command comes back as [`Outcome::Confirm`], and only an install
//! repeated with its hash runs it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::process::command;

/// Reading the lists is local, and quick.
const LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// A change may clone a repository. `claude` gives a clone two minutes of its
/// own, so this only catches one that hangs outright.
const CHANGE_TIMEOUT: Duration = Duration::from_secs(300);

/// Everything the `/plugin` window shows for one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalog {
    pub installed: Vec<Installed>,
    /// What the marketplaces offer that isn't installed yet.
    pub available: Vec<Available>,
    pub marketplaces: Vec<Marketplace>,
}

/// A plugin installed in some scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// `name@marketplace`.
    pub id: String,
    #[serde(default)]
    pub version: String,
    /// `user`, `project` or `local`: where it was installed, and so where it
    /// has to be enabled, disabled or removed.
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub enabled: bool,
    /// From the plugin's own manifest, since the marketplace listing leaves
    /// installed plugins out.
    #[serde(default)]
    pub description: String,
    #[serde(default, rename(deserialize = "installPath"), skip_serializing)]
    install_path: Option<PathBuf>,
}

/// A plugin a marketplace offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Available {
    /// `name@marketplace`, what an install names.
    #[serde(rename(deserialize = "pluginId"))]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename(deserialize = "marketplaceName"))]
    pub marketplace: String,
    /// How many times it has been installed, where the marketplace counts.
    #[serde(default, rename(deserialize = "installCount"))]
    pub installs: Option<u64>,
}

/// A marketplace Claude Code has been told about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Marketplace {
    pub name: String,
    /// What kind of place it comes from: `github`, `git`, `url`, `directory`.
    pub source: String,
    /// Where: `owner/repo`, a URL or a path.
    pub location: String,
}

/// One change the window can ask for. Scopes are the plugin's own, as the
/// catalog listed it; `accept_command` is the hash of a marketplace-declared
/// command the user has seen and agreed to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Change {
    Install {
        plugin: String,
        #[serde(default)]
        accept_command: Option<String>,
    },
    Uninstall {
        plugin: String,
        scope: String,
    },
    Enable {
        plugin: String,
        scope: String,
    },
    Disable {
        plugin: String,
        scope: String,
    },
    Update {
        plugin: String,
        scope: String,
        #[serde(default)]
        accept_command: Option<String>,
    },
    AddMarketplace {
        source: String,
    },
    RemoveMarketplace {
        name: String,
    },
    /// Fetch one marketplace's listing afresh, or every one's with `None`.
    UpdateMarketplace {
        #[serde(default)]
        name: Option<String>,
    },
}

/// How a change went, when it didn't fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Done, in `claude`'s words.
    Done { message: String },
    /// Not done: the plugin's marketplace wants to run `command`. Asking again
    /// with `sha256` as the `accept_command` runs it.
    Confirm {
        message: String,
        command: String,
        sha256: String,
    },
}

/// The plugins installed and on offer, and the marketplaces, as `claude` sees
/// them from `cwd`. A Project's own plugins and marketplaces are in its
/// settings, so the directory matters.
pub async fn catalog(bin: &str, cwd: &Path) -> Result<Catalog> {
    let (plugins, marketplaces) = tokio::try_join!(
        run(
            bin,
            cwd,
            &["plugin", "list", "--json", "--available"],
            LIST_TIMEOUT
        ),
        run(
            bin,
            cwd,
            &["plugin", "marketplace", "list", "--json"],
            LIST_TIMEOUT
        ),
    )?;
    for out in [&plugins, &marketplaces] {
        if !out.ok {
            return Err(Error::Plugins(out.complaint()));
        }
    }
    let mut catalog = parse_catalog(&plugins.stdout, &marketplaces.stdout)?;
    for plugin in &mut catalog.installed {
        plugin.description = plugin
            .install_path
            .as_deref()
            .map(manifest_description)
            .unwrap_or_default();
    }
    Ok(catalog)
}

/// Make one change, from `cwd`.
pub async fn change(bin: &str, cwd: &Path, change: &Change) -> Result<Outcome> {
    let args = args(change);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run(bin, cwd, &args, CHANGE_TIMEOUT).await?;
    if json_answer(change) {
        return read_outcome(&out);
    }
    // The marketplace commands have no JSON to give: their word is the last
    // line they print, and the exit status says which way it went.
    if out.ok {
        Ok(Outcome::Done {
            message: last_line(&out.stdout).unwrap_or_else(|| "Done.".into()),
        })
    } else {
        Err(Error::Plugins(out.complaint()))
    }
}

/// The `claude` arguments that make `change`. Never `-y`: a command a
/// marketplace declares is only run once the user has seen it, by its hash.
/// What the change is about comes last, after `--`, so a name the user typed
/// can't be taken for a flag.
fn args(change: &Change) -> Vec<String> {
    let v = |parts: &[&str]| parts.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (mut args, target) = match change {
        Change::Install {
            plugin,
            accept_command,
        } => {
            let mut args = v(&["plugin", "install", "--json", "--scope", "user"]);
            accepting(&mut args, accept_command);
            (args, Some(plugin))
        }
        Change::Uninstall { plugin, scope } => (
            v(&["plugin", "uninstall", "--json", "--scope", scope]),
            Some(plugin),
        ),
        Change::Enable { plugin, scope } => (
            v(&["plugin", "enable", "--json", "--scope", scope]),
            Some(plugin),
        ),
        Change::Disable { plugin, scope } => (
            v(&["plugin", "disable", "--json", "--scope", scope]),
            Some(plugin),
        ),
        Change::Update {
            plugin,
            scope,
            accept_command,
        } => {
            let mut args = v(&["plugin", "update", "--json", "--scope", scope]);
            accepting(&mut args, accept_command);
            (args, Some(plugin))
        }
        Change::AddMarketplace { source } => (v(&["plugin", "marketplace", "add"]), Some(source)),
        Change::RemoveMarketplace { name } => (v(&["plugin", "marketplace", "remove"]), Some(name)),
        Change::UpdateMarketplace { name } => {
            (v(&["plugin", "marketplace", "update"]), name.as_ref())
        }
    };
    if let Some(target) = target {
        args.extend(["--".into(), target.clone()]);
    }
    args
}

fn accepting(args: &mut Vec<String>, sha: &Option<String>) {
    if let Some(sha) = sha {
        args.extend(["--accept-command".into(), sha.clone()]);
    }
}

/// Whether `claude` answers `change` with a line of JSON.
fn json_answer(change: &Change) -> bool {
    !matches!(
        change,
        Change::AddMarketplace { .. }
            | Change::RemoveMarketplace { .. }
            | Change::UpdateMarketplace { .. }
    )
}

/// What a plugin command's JSON line says happened.
fn read_outcome(out: &Output) -> Result<Outcome> {
    let Some(answer) = out
        .stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
    else {
        return Err(Error::Plugins(out.complaint()));
    };
    let text = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_string);
    let message = text("message").unwrap_or_default();
    if text("outcome").as_deref() == Some("ok") {
        return Ok(Outcome::Done { message });
    }
    // Asking for what's already so — enabling an enabled plugin — isn't worth
    // an error.
    if answer.get("alreadyInGoalState").and_then(Value::as_bool) == Some(true) {
        return Ok(Outcome::Done { message });
    }
    if let Some(shown) = answer.get("shownCommand") {
        if let Some(sha256) = shown.get("sha256").and_then(Value::as_str) {
            let command = match shown.get("command") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .map(|p| {
                        p.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| p.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            return Ok(Outcome::Confirm {
                message,
                command,
                sha256: sha256.to_string(),
            });
        }
    }
    Err(Error::Plugins(if message.is_empty() {
        out.complaint()
    } else {
        message
    }))
}

fn parse_catalog(plugins: &str, marketplaces: &str) -> Result<Catalog> {
    #[derive(Deserialize)]
    struct Listed {
        #[serde(default)]
        installed: Vec<Installed>,
        #[serde(default)]
        available: Vec<Available>,
    }
    #[derive(Deserialize)]
    struct Declared {
        name: String,
        #[serde(default)]
        source: String,
        repo: Option<String>,
        url: Option<String>,
        path: Option<String>,
    }
    let listed: Listed = serde_json::from_str(plugins)?;
    let declared: Vec<Declared> = serde_json::from_str(marketplaces)?;
    let mut available = listed.available;
    // Most installed first, the way Claude Code's Discover lists them.
    available.sort_by(|a, b| {
        b.installs
            .unwrap_or(0)
            .cmp(&a.installs.unwrap_or(0))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Catalog {
        installed: listed.installed,
        available,
        marketplaces: declared
            .into_iter()
            .map(|m| Marketplace {
                location: m.repo.or(m.url).or(m.path).unwrap_or_default(),
                name: m.name,
                source: m.source,
            })
            .collect(),
    })
}

/// The description in an installed plugin's `.claude-plugin/plugin.json`, or
/// nothing if it has none to give.
fn manifest_description(install_path: &Path) -> String {
    std::fs::read_to_string(install_path.join(".claude-plugin/plugin.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("description")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn last_line(text: &str) -> Option<String> {
    text.lines().map(tidy).rfind(|l| !l.is_empty())
}

/// A line of `claude`'s human output without its tick or cross, and without
/// the progress it printed before it on the same line.
fn tidy(line: &str) -> String {
    let line = line.rsplit(['✔', '✘']).next().unwrap_or(line);
    line.trim().to_string()
}

struct Output {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Output {
    /// Why the command failed, in the words it used.
    fn complaint(&self) -> String {
        let said = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        let lines: Vec<String> = said.lines().map(tidy).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            "`claude plugin` failed without saying why".into()
        } else {
            lines.join("\n")
        }
    }
}

async fn run(bin: &str, cwd: &Path, args: &[&str], limit: Duration) -> Result<Output> {
    let fail = |why: String| Error::Plugins(why);
    let mut cmd = command(bin);
    cmd.args(args)
        .current_dir(cwd)
        // No one to answer a prompt: a question is a refusal.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::process::outside_appimage(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| fail(format!("could not start `claude`: {e}")))?;
    let out = tokio::time::timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| fail("`claude plugin` took too long to answer".into()))?
        .map_err(|e| fail(e.to_string()))?;
    Ok(Output {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real `claude plugin list --json --available`.
    const LISTED: &str = r#"{"installed":[
        {"id":"brag@brag","version":"0.4.0","scope":"user","enabled":true,"installPath":"/nowhere/brag","installedAt":"2026-09-25T12:10:12.552Z"}
    ],"available":[
        {"pluginId":"tiny@official","name":"tiny","description":"Small","marketplaceName":"official","source":"./plugins/tiny","installCount":12},
        {"pluginId":"big@official","name":"big","description":"Popular","marketplaceName":"official","source":{"source":"git-subdir","url":"https://example.com/x.git"},"installCount":70287},
        {"pluginId":"new@official","name":"new","description":"Uncounted","marketplaceName":"official","source":"./plugins/new"}
    ]}"#;

    const MARKETPLACES: &str = r#"[
        {"name":"official","source":"github","repo":"anthropics/claude-plugins-official","installLocation":"/x"},
        {"name":"mine","source":"directory","path":"/home/u/market","installLocation":"/y"}
    ]"#;

    #[test]
    fn reads_the_catalog_most_installed_first() {
        let catalog = parse_catalog(LISTED, MARKETPLACES).unwrap();
        assert_eq!(catalog.installed[0].id, "brag@brag");
        assert!(catalog.installed[0].enabled);
        assert_eq!(catalog.installed[0].scope, "user");
        let ids: Vec<&str> = catalog.available.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["big@official", "tiny@official", "new@official"]);
        assert_eq!(catalog.available[0].installs, Some(70287));
        assert_eq!(catalog.available[2].installs, None);
        assert_eq!(
            catalog.marketplaces[0].location,
            "anthropics/claude-plugins-official"
        );
        assert_eq!(catalog.marketplaces[1].location, "/home/u/market");
    }

    #[test]
    fn reads_an_installed_plugins_description_from_its_manifest() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            dir.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"brag","description":"Launch videos"}"#,
        )
        .unwrap();
        assert_eq!(manifest_description(dir.path()), "Launch videos");
        assert_eq!(manifest_description(&dir.path().join("gone")), "");
    }

    #[test]
    fn installs_for_the_user_and_never_says_yes_for_them() {
        let install = Change::Install {
            plugin: "big@official".into(),
            accept_command: None,
        };
        assert_eq!(
            args(&install).join(" "),
            "plugin install --json --scope user -- big@official"
        );
        let accepted = Change::Install {
            plugin: "big@official".into(),
            accept_command: Some("abc".into()),
        };
        assert!(args(&accepted)
            .join(" ")
            .ends_with("--accept-command abc -- big@official"));
        assert!(!args(&accepted).iter().any(|a| a == "-y" || a == "--yes"));
    }

    #[test]
    fn a_typed_name_is_never_a_flag() {
        let change = Change::RemoveMarketplace {
            name: "--scope".into(),
        };
        assert_eq!(
            args(&change).join(" "),
            "plugin marketplace remove -- --scope"
        );
    }

    #[test]
    fn changes_to_an_installed_plugin_go_to_its_own_scope() {
        let change = Change::Disable {
            plugin: "x@m".into(),
            scope: "project".into(),
        };
        assert_eq!(
            args(&change).join(" "),
            "plugin disable --json --scope project -- x@m"
        );
        let all = Change::UpdateMarketplace { name: None };
        assert_eq!(args(&all).join(" "), "plugin marketplace update");
    }

    #[test]
    fn the_window_names_changes_as_it_sends_them() {
        let change: Change =
            serde_json::from_str(r#"{"do":"install","plugin":"x@m","acceptCommand":"abc"}"#)
                .unwrap();
        assert_eq!(
            change,
            Change::Install {
                plugin: "x@m".into(),
                accept_command: Some("abc".into())
            }
        );
        let change: Change =
            serde_json::from_str(r#"{"do":"add_marketplace","source":"owner/repo"}"#).unwrap();
        assert_eq!(
            change,
            Change::AddMarketplace {
                source: "owner/repo".into()
            }
        );
    }

    fn output(ok: bool, stdout: &str, stderr: &str) -> Output {
        Output {
            ok,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn an_ok_answer_is_done_in_claudes_words() {
        let out = output(
            true,
            r#"{"command":"install","outcome":"ok","plugin":"x@m","scope":"user","message":"Successfully installed plugin: x@m (scope: user)"}"#,
            "",
        );
        assert_eq!(
            read_outcome(&out).unwrap(),
            Outcome::Done {
                message: "Successfully installed plugin: x@m (scope: user)".into()
            }
        );
    }

    #[test]
    fn a_failure_carries_its_message() {
        let out = output(
            false,
            r#"{"command":"install","outcome":"failed","plugin":"nope","message":"Plugin \"nope\" not found in any configured marketplace","failureCode":"not_found"}"#,
            "✘ Failed to install plugin \"nope\": ...",
        );
        let err = read_outcome(&out).unwrap_err().to_string();
        assert!(
            err.contains("not found in any configured marketplace"),
            "{err}"
        );
    }

    #[test]
    fn asking_for_what_is_already_so_is_not_an_error() {
        let out = output(
            false,
            r#"{"command":"disable","outcome":"failed","message":"Plugin \"x@m\" is already disabled","failureCode":"already_in_goal_state","alreadyInGoalState":true}"#,
            "",
        );
        assert!(matches!(read_outcome(&out).unwrap(), Outcome::Done { .. }));
    }

    #[test]
    fn a_declared_command_comes_back_to_be_confirmed() {
        let out = output(
            false,
            r#"{"command":"install","outcome":"failed","plugin":"x@m","message":"Review the command","failureCode":"command_source_refused","shownCommand":{"kind":"command_source","pluginId":"x@m","command":"npx some-installer","sha256":"f00d"}}"#,
            "",
        );
        assert_eq!(
            read_outcome(&out).unwrap(),
            Outcome::Confirm {
                message: "Review the command".into(),
                command: "npx some-installer".into(),
                sha256: "f00d".into()
            }
        );
    }

    #[test]
    fn a_marketplace_complaint_loses_its_progress_and_cross() {
        let out = output(
            false,
            "",
            "Adding marketplace…✘ Failed to add marketplace: not found\n",
        );
        assert_eq!(out.complaint(), "Failed to add marketplace: not found");
        assert_eq!(
            last_line("Updating marketplace: m...✔ Successfully updated marketplace: m\n"),
            Some("Successfully updated marketplace: m".into())
        );
    }

    /// The whole exchange against a stand-in `claude` that answers each
    /// subcommand as the real one does.
    #[cfg(unix)]
    #[tokio::test]
    async fn asks_claude_for_the_catalog_and_to_install() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("claude");
        let script = format!(
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'plugin list --json --available') cat <<'EOF'\n{}\nEOF\n;;\n\
             'plugin marketplace list --json') cat <<'EOF'\n{}\nEOF\n;;\n\
             'plugin install --json --scope user -- big@official') echo '{{\"outcome\":\"ok\",\"message\":\"Installed big\"}}' ;;\n\
             'plugin marketplace add -- owner/repo') echo 'Adding…✔ Added owner/repo' ;;\n\
             *) echo \"unexpected: $*\" >&2; exit 9 ;;\n\
             esac\n",
            LISTED.replace('\n', " "),
            MARKETPLACES.replace('\n', " "),
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let bin = bin.to_str().unwrap();

        let catalog = catalog(bin, dir.path()).await.unwrap();
        assert_eq!(catalog.available.len(), 3);
        assert_eq!(catalog.marketplaces.len(), 2);

        let install = Change::Install {
            plugin: "big@official".into(),
            accept_command: None,
        };
        assert_eq!(
            change(bin, dir.path(), &install).await.unwrap(),
            Outcome::Done {
                message: "Installed big".into()
            }
        );
        let add = Change::AddMarketplace {
            source: "owner/repo".into(),
        };
        assert_eq!(
            change(bin, dir.path(), &add).await.unwrap(),
            Outcome::Done {
                message: "Added owner/repo".into()
            }
        );
        let remove = Change::RemoveMarketplace {
            name: "nope".into(),
        };
        let err = change(bin, dir.path(), &remove).await.unwrap_err();
        assert!(err.to_string().contains("unexpected"), "{err}");
    }
}
