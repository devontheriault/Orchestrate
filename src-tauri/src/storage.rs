use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Agent, AgentEvent, Project};
use crate::error::{Error, Result};
use crate::paths;

/// The app's persistent state: the registered Projects. In-flight Agents are
/// not stored here — they live as per-Agent meta files under `logs/`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub projects: Vec<Project>,
}

impl Registry {
    /// Load from `state_file()`. Missing file returns an empty Registry.
    pub fn load() -> Result<Self> {
        let path = paths::state_file()?;
        match fs::read_to_string(&path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::Io { path, source }),
        }
    }

    /// Save atomically (write-and-rename).
    pub fn save(&self) -> Result<()> {
        paths::ensure_dirs()?;
        let path = paths::state_file()?;
        let contents = serde_json::to_string_pretty(self)?;
        atomic_write(&path, &contents)
    }

    /// The registered Project with this id, if there still is one.
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
}

/// Read one Agent's meta file.
pub fn load_agent(agent_id: &str) -> Result<Agent> {
    let path = paths::agent_meta_path(agent_id)?;
    let s = fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    Ok(serde_json::from_str(&s)?)
}

/// Write an Agent's meta file atomically.
pub fn save_agent(agent: &Agent) -> Result<()> {
    paths::ensure_dirs()?;
    let path = paths::agent_meta_path(&agent.id)?;
    let contents = serde_json::to_string_pretty(agent)?;
    atomic_write(&path, &contents)
}

/// Leave the ids of the Agents a stopping Host Orphaned for the next one.
pub fn save_orphan_ids(ids: &[String]) -> Result<()> {
    paths::ensure_dirs()?;
    atomic_write(&paths::orphans_file()?, &serde_json::to_string(ids)?)
}

/// The ids the last Host left with [`save_orphan_ids`], taken so they are
/// reported once.
pub fn take_orphan_ids() -> Result<Vec<String>> {
    let path = paths::orphans_file()?;
    let ids = match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(source) => return Err(Error::Io { path, source }),
    };
    let _ = fs::remove_file(&path);
    Ok(ids)
}

/// Append one event as a JSONL line to the Agent's log file. Called for every
/// line an Agent prints, so the folders are only made when the log can't be
/// opened without them, and the line goes in one write, never without its end.
pub fn append_event(agent_id: &str, event: &AgentEvent) -> Result<()> {
    let path = paths::agent_log_path(agent_id)?;
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    let open = || fs::OpenOptions::new().create(true).append(true).open(&path);
    let opened = match open() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            paths::ensure_dirs()?;
            open()
        }
        opened => opened,
    };
    opened
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|source| Error::Io { path, source })
}

/// Replay an Agent's event log, oldest first, less what no window shows (see
/// [`unseen`]). A missing log means the Agent has not produced output yet,
/// which is not an error. Lines that fail to parse are skipped: the last line
/// of a live log can be half-written. Logs written before [`slim`] existed are
/// slimmed on the way out.
pub fn read_events(agent_id: &str) -> Result<Vec<AgentEvent>> {
    let path = paths::agent_log_path(agent_id)?;
    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Io { path, source }),
    };
    Ok(contents
        .lines()
        .filter_map(|line| serde_json::from_str::<AgentEvent>(line).ok())
        .filter(|e| !unseen(&e.event))
        .map(|mut e| {
            slim(&mut e.event);
            e
        })
        .collect())
}

/// The final answer of the Agent's last Turn that gave one, from its log.
/// `None` until a Turn has ended with one.
///
/// Read from the end, and only lines that could hold one are parsed: a long
/// log runs to megabytes, and the answer is near its end.
pub fn last_answer(agent_id: &str) -> Option<String> {
    let contents = fs::read_to_string(paths::agent_log_path(agent_id).ok()?).ok()?;
    contents
        .lines()
        .rev()
        .filter(|line| line.contains("\"result\""))
        .filter_map(|line| serde_json::from_str::<AgentEvent>(line).ok())
        .find_map(|e| {
            (e.event.get("type")?.as_str()? == "result")
                .then(|| e.event.get("result")?.as_str())
                .flatten()
                .filter(|r| !r.trim().is_empty())
                .map(str::to_owned)
        })
}

/// Whether a `claude` event is telemetry no window shows: the running count
/// of thinking tokens, sent about once a second while the model thinks, and
/// the account's rate limits. They stay in the log, where the usage totals
/// read the limits, but aren't sent to a window or replayed to one. Most of a
/// thinking Agent's events are these, and each one cost a window a rebuild
/// of the transcript it would leave unchanged.
pub fn unseen(event: &serde_json::Value) -> bool {
    let kind = event.get("type").and_then(|t| t.as_str());
    kind == Some("rate_limit_event")
        || (kind == Some("system")
            && event.get("subtype").and_then(|s| s.as_str()) == Some("thinking_tokens"))
}

/// Drop the parts of a `claude` event the transcript never shows, which are
/// also by far its heaviest. Every tool result arrives twice — in the message,
/// and again as `tool_use_result` — and an image or PDF the Agent read comes
/// inline as base64, often most of a megabyte a screenshot. The block itself
/// stays, less its bytes, so the transcript can still say one was there.
pub fn slim(event: &mut serde_json::Value) {
    use serde_json::Value;
    match event {
        Value::Object(map) => {
            map.remove("tool_use_result");
            if let Some(Value::Object(source)) = map.get_mut("source") {
                if source.get("type").and_then(Value::as_str) == Some("base64") {
                    source.remove("data");
                }
            }
            map.values_mut().for_each(slim);
        }
        Value::Array(items) => items.iter_mut().for_each(slim),
        _ => {}
    }
}

/// List all Agents on disk by scanning meta files. Order is unspecified.
pub fn list_agents() -> Result<Vec<Agent>> {
    let dir = paths::logs_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Io { path: dir, source }),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.ends_with(".meta"))
        {
            let contents = fs::read_to_string(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            out.push(serde_json::from_str(&contents)?);
        }
    }
    Ok(out)
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    write_whole(path, contents.as_bytes(), false).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// Write `bytes` to `path` all at once: into a hidden file beside it, flushed
/// to disk, then renamed over it. A reader sees the old contents or the new,
/// never half of either, and neither does what's left after a crash.
///
/// With `private` the file is the user's alone from its first byte, so a
/// secret is never readable by anyone else even for a moment. On Windows a
/// file in the user's own profile is private to them already. Otherwise a
/// file that was there keeps its permissions.
pub fn write_whole(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder to write into"))?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{:08x}.tmp", rand::random::<u32>()));
    let written = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if !private {
            if let Ok(meta) = fs::metadata(path) {
                fs::set_permissions(&tmp, meta.permissions())?;
            }
        }
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{new_id, AgentState, Task};
    use crate::test_util::StateEnv;
    use time::OffsetDateTime;

    fn sample_agent() -> Agent {
        Agent {
            id: new_id(),
            project_id: new_id(),
            task: Task {
                prompt: "test".into(),
                attachments: vec![],
            },
            state: AgentState::Running,
            worktree_path: "/tmp/wt".into(),
            base_commit: None,
            session_id: Some("7c9e6679-7425-40de-944b-e07fc1f90ae7".into()),
            model: None,
            effort: None,
            permission_mode: None,
            read_mail: false,
            options: Default::default(),
            turns: 1,
            title: None,
            user_title: None,
            color: None,
            branch: "cw/agent-test".into(),
            spawned_at: OffsetDateTime::now_utc(),
            turn_started_at: Some(OffsetDateTime::now_utc()),
            exited_at: None,
            exit_code: None,
            fail_reason: None,
            merged_branch: None,
            merged_at: None,
            unpushed: false,
            push_error: None,
            resolves: None,
            lead_id: None,
            queue: vec![],
        }
    }

    #[test]
    fn registry_missing_file_returns_default() {
        let _env = StateEnv::new();
        let r = Registry::load().unwrap();
        assert!(r.projects.is_empty());
    }

    #[test]
    fn registry_roundtrips_via_disk() {
        let _env = StateEnv::new();
        let r = Registry {
            projects: vec![Project {
                id: "p".into(),
                name: "N".into(),
                path: "/p".into(),
                added_at: OffsetDateTime::now_utc(),
                cloned: false,
            }],
        };
        r.save().unwrap();
        let back = Registry::load().unwrap();
        assert_eq!(r.projects.len(), back.projects.len());
        assert_eq!(r.projects[0].id, back.projects[0].id);
    }

    #[test]
    fn agent_meta_roundtrips() {
        let _env = StateEnv::new();
        let a = sample_agent();
        save_agent(&a).unwrap();
        let back = load_agent(&a.id).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn append_and_list_events() {
        let _env = StateEnv::new();
        let a = sample_agent();
        for i in 0..3 {
            let e = AgentEvent {
                ts: OffsetDateTime::now_utc(),
                event: serde_json::json!({"n": i}),
            };
            append_event(&a.id, &e).unwrap();
        }
        let log = fs::read_to_string(paths::agent_log_path(&a.id).unwrap()).unwrap();
        assert_eq!(log.lines().count(), 3);
    }

    #[test]
    fn read_events_replays_in_order() {
        let _env = StateEnv::new();
        let a = sample_agent();
        for i in 0..3 {
            let e = AgentEvent {
                ts: OffsetDateTime::now_utc(),
                event: serde_json::json!({"n": i}),
            };
            append_event(&a.id, &e).unwrap();
        }
        let back = read_events(&a.id).unwrap();
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].event, serde_json::json!({"n": 0}));
        assert_eq!(back[2].event, serde_json::json!({"n": 2}));
    }

    #[test]
    fn read_events_empty_when_no_log() {
        let _env = StateEnv::new();
        assert!(read_events("nope").unwrap().is_empty());
    }

    #[test]
    fn read_events_skips_a_half_written_line() {
        let _env = StateEnv::new();
        let a = sample_agent();
        let e = AgentEvent {
            ts: OffsetDateTime::now_utc(),
            event: serde_json::json!({"n": 0}),
        };
        append_event(&a.id, &e).unwrap();
        let path = paths::agent_log_path(&a.id).unwrap();
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"ts\":\"2026-09-20T14:00:00").unwrap();
        assert_eq!(read_events(&a.id).unwrap().len(), 1);
    }

    /// A Read of a screenshot, as `claude` streams it: the image inline in the
    /// tool result, and the whole result repeated as `tool_use_result`.
    fn screenshot_result() -> serde_json::Value {
        let image = serde_json::json!({
            "type": "image",
            "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="},
        });
        serde_json::json!({
            "type": "user",
            "message": {"content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [image.clone(), {"type": "text", "text": "a caption"}],
            }]},
            "tool_use_result": [image],
        })
    }

    #[test]
    fn slim_drops_duplicate_results_and_inline_bytes() {
        let mut e = screenshot_result();
        slim(&mut e);
        assert_eq!(
            e,
            serde_json::json!({
                "type": "user",
                "message": {"content": [{
                    "type": "tool_result",
                    "tool_use_id": "t1",
                    "content": [
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png"}},
                        {"type": "text", "text": "a caption"},
                    ],
                }]},
            })
        );
    }

    #[test]
    fn slim_leaves_text_alone() {
        let mut e = serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "data: base64"}]},
            "source": "cli",
        });
        let before = e.clone();
        slim(&mut e);
        assert_eq!(e, before);
    }

    #[test]
    fn read_events_slims_a_log_written_before_slimming() {
        let _env = StateEnv::new();
        let a = sample_agent();
        let e = AgentEvent {
            ts: OffsetDateTime::now_utc(),
            event: screenshot_result(),
        };
        append_event(&a.id, &e).unwrap();
        let back = read_events(&a.id).unwrap();
        assert!(back[0].event.get("tool_use_result").is_none());
        assert!(!back[0].event.to_string().contains("iVBORw0KGgo="));
    }

    #[test]
    fn read_events_leaves_out_what_no_window_shows() {
        let _env = StateEnv::new();
        let a = sample_agent();
        for event in [
            serde_json::json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": 50}),
            serde_json::json!({"type": "rate_limit_event", "rate_limit_info": {}}),
            serde_json::json!({"type": "system", "subtype": "init"}),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "thinking_tokens"}]}}),
        ] {
            let e = AgentEvent {
                ts: OffsetDateTime::now_utc(),
                event,
            };
            append_event(&a.id, &e).unwrap();
        }
        let back = read_events(&a.id).unwrap();
        let kinds: Vec<&str> = back
            .iter()
            .map(|e| e.event["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["system", "assistant"]);
        // Still on disk, where the usage totals read the limits.
        let log = fs::read_to_string(paths::agent_log_path(&a.id).unwrap()).unwrap();
        assert_eq!(log.lines().count(), 4);
    }

    #[test]
    fn list_agents_empty_when_no_dir() {
        let _env = StateEnv::new();
        assert!(list_agents().unwrap().is_empty());
    }

    #[test]
    fn list_agents_returns_saved() {
        let _env = StateEnv::new();
        let a = sample_agent();
        save_agent(&a).unwrap();
        let listed = list_agents().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, a.id);
    }
}
