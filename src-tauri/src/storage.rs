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

/// Append one event as a JSONL line to the Agent's log file.
pub fn append_event(agent_id: &str, event: &AgentEvent) -> Result<()> {
    paths::ensure_dirs()?;
    let path = paths::agent_log_path(agent_id)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
    let line = serde_json::to_string(event)?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|source| Error::Io { path, source })
}

/// Replay an Agent's event log, oldest first. A missing log means the Agent
/// has not produced output yet, which is not an error. Lines that fail to parse
/// are skipped: the last line of a live log can be half-written. Logs written
/// before [`slim`] existed are slimmed on the way out.
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
        .map(|mut e| {
            slim(&mut e.event);
            e
        })
        .collect())
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
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents).map_err(|source| Error::Io {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
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
            resolves: None,
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
