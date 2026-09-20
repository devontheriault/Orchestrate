//! What the Agents have spent: tokens and money per Model, and how much of the
//! account's rate-limit windows Claude Code last reported.
//!
//! Both numbers come out of the Agent logs already on disk rather than a
//! counter kept in memory. `claude` reports a Turn's per-model usage on its
//! `result` event and the account's limit windows on `rate_limit_event`, so
//! the logs are the record: reading them means the totals survive a restart,
//! and they cover Agents this window never opened — Reaped ones included,
//! whose logs outlive them.

use std::collections::HashMap;
use std::fs;

use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

use crate::error::{Error, Result};
use crate::model::AgentEvent;
use crate::paths;

/// Cheap prefilters. Parsing every line of every log as JSON costs far more
/// than scanning for the two shapes that carry usage at all — most lines are
/// assistant text we don't need here.
const RESULT_MARKER: &str = "\"modelUsage\"";
const LIMIT_MARKER: &str = "\"rate_limit_event\"";

/// The order the known limit windows read in: shortest first, the way a user
/// thinks about them. Anything Claude Code adds later sorts after these.
const WINDOW_ORDER: [&str; 3] = ["five_hour", "seven_day", "seven_day_opus"];

/// What one Model cost across the Turns that used it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModelUsage {
    /// The model as `--model` names it, e.g. `claude-opus-4-5-20251101`.
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    /// Claude Code's own cost figure for those tokens, in USD.
    pub cost_usd: f64,
    /// The model's context window, as Claude Code reported it.
    pub context_window: Option<u64>,
}

/// One Agent's share of the total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentUsage {
    pub agent_id: String,
    /// Per Model, biggest spender first.
    pub models: Vec<ModelUsage>,
    /// `result` events counted — one per Turn that reached an answer.
    pub turns: u32,
    /// When the last of those landed.
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_at: Option<OffsetDateTime>,
}

/// One rate-limit window on the account.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimitWindow {
    /// Claude Code's name for it: `five_hour`, `seven_day`, …
    pub kind: String,
    /// How much of the window is spent, 0.0–1.0.
    pub utilization: f64,
    /// Unix seconds at which it rolls over.
    pub resets_at: i64,
}

/// The account's limits as of the last time an Agent was told about them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limits {
    pub windows: Vec<LimitWindow>,
    /// When that Agent heard it — these are a snapshot, not a live reading.
    #[serde(with = "time::serde::rfc3339")]
    pub observed_at: OffsetDateTime,
    pub using_overage: bool,
    /// `allowed`, `rejected`, … as Claude Code reported it.
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UsageSummary {
    /// Agents that have spent anything, costliest first.
    pub agents: Vec<AgentUsage>,
    /// `None` until some Agent has been told its limits — a fresh install, or
    /// an account the API doesn't meter this way.
    pub limits: Option<Limits>,
}

/// Read every Agent log and total up what it spent.
pub fn summary() -> Result<UsageSummary> {
    let dir = paths::logs_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(UsageSummary::default()),
        Err(source) => return Err(Error::Io { path: dir, source }),
    };

    let mut agents = Vec::new();
    let mut limits: Option<Limits> = None;

    for entry in entries {
        let path = entry
            .map_err(|source| Error::Io {
                path: dir.clone(),
                source,
            })?
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(agent_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // A log being unreadable shouldn't cost the user the whole view; the
        // Agents that did read give an honest partial total.
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };

        let (usage, seen) = scan_log(agent_id, &contents);
        if let Some(seen) = seen {
            let newer = limits
                .as_ref()
                .is_none_or(|kept| seen.observed_at > kept.observed_at);
            if newer {
                limits = Some(seen);
            }
        }
        if usage.turns > 0 {
            agents.push(usage);
        }
    }

    agents.sort_by(|a, b| {
        total_cost(b)
            .partial_cmp(&total_cost(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    Ok(UsageSummary { agents, limits })
}

fn total_cost(a: &AgentUsage) -> f64 {
    a.models.iter().map(|m| m.cost_usd).sum()
}

/// Total one Agent's log: its per-Model spend, plus the newest limits reading
/// it happens to carry.
fn scan_log(agent_id: &str, contents: &str) -> (AgentUsage, Option<Limits>) {
    let mut models: HashMap<String, ModelUsage> = HashMap::new();
    let mut turns = 0u32;
    let mut last_at: Option<OffsetDateTime> = None;
    let mut limits: Option<Limits> = None;

    for line in contents.lines() {
        let is_result = line.contains(RESULT_MARKER);
        let is_limit = line.contains(LIMIT_MARKER);
        if !is_result && !is_limit {
            continue;
        }
        // A half-written last line is normal on a live log.
        let Ok(event) = serde_json::from_str::<AgentEvent>(line) else {
            continue;
        };

        if is_result {
            if let Some(usage) = event.event.get("modelUsage") {
                fold_models(&mut models, usage);
                turns += 1;
                last_at = Some(event.ts);
            }
        }
        if is_limit {
            if let Some(seen) = parse_limits(&event) {
                let newer = limits
                    .as_ref()
                    .is_none_or(|kept| seen.observed_at > kept.observed_at);
                if newer {
                    limits = Some(seen);
                }
            }
        }
    }

    let mut models: Vec<ModelUsage> = models.into_values().collect();
    models.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });

    (
        AgentUsage {
            agent_id: agent_id.to_owned(),
            models,
            turns,
            last_at,
        },
        limits,
    )
}

/// Add one `result` event's `modelUsage` map to the running per-Model totals.
fn fold_models(totals: &mut HashMap<String, ModelUsage>, usage: &Value) {
    let Some(map) = usage.as_object() else { return };
    for (model, v) in map {
        let entry = totals.entry(model.clone()).or_insert_with(|| ModelUsage {
            model: model.clone(),
            ..ModelUsage::default()
        });
        entry.input_tokens += u64_at(v, "inputTokens");
        entry.output_tokens += u64_at(v, "outputTokens");
        entry.cache_read_tokens += u64_at(v, "cacheReadInputTokens");
        entry.cache_creation_tokens += u64_at(v, "cacheCreationInputTokens");
        entry.cost_usd += v.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0);
        if let Some(w) = v.get("contextWindow").and_then(Value::as_u64) {
            entry.context_window = Some(w);
        }
    }
}

fn u64_at(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn parse_limits(event: &AgentEvent) -> Option<Limits> {
    let info = event.event.get("rate_limit_info")?;
    let mut windows: Vec<LimitWindow> = info
        .get("unifiedWindows")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(kind, w)| {
                    Some(LimitWindow {
                        kind: kind.clone(),
                        utilization: w.get("utilization").and_then(Value::as_f64)?,
                        resets_at: w.get("resetsAt").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if windows.is_empty() {
        return None;
    }
    windows.sort_by_key(|w| {
        (
            WINDOW_ORDER
                .iter()
                .position(|k| *k == w.kind)
                .unwrap_or(WINDOW_ORDER.len()),
            w.kind.clone(),
        )
    });

    Some(Limits {
        windows,
        observed_at: event.ts,
        using_overage: info
            .get("isUsingOverage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        status: info
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::StateEnv;
    use serde_json::json;
    use time::macros::datetime;

    /// A JSONL log built from timestamped events, in the order given.
    fn log(lines: &[(&str, Value)]) -> String {
        lines
            .iter()
            .map(|(ts, event)| format!("{}\n", json!({ "ts": ts, "event": event })))
            .collect()
    }

    fn turn(model: &str, input: u64, output: u64, cost: f64) -> Value {
        json!({
            "type": "result",
            "subtype": "success",
            "modelUsage": {
                model: {
                    "inputTokens": input,
                    "outputTokens": output,
                    "cacheReadInputTokens": 10,
                    "cacheCreationInputTokens": 5,
                    "costUSD": cost,
                    "contextWindow": 200000,
                }
            }
        })
    }

    fn limits(five: f64, seven: f64) -> Value {
        json!({
            "type": "rate_limit_event",
            "rate_limit_info": {
                "isUsingOverage": false,
                "status": "allowed",
                "unifiedWindows": {
                    "seven_day": { "resetsAt": 1789977600, "utilization": seven },
                    "five_hour": { "resetsAt": 1789947600, "utilization": five },
                }
            }
        })
    }

    #[test]
    fn sums_tokens_per_model_across_turns() {
        let log = log(&[
            ("2026-09-20T10:00:00Z", turn("claude-opus-5", 100, 20, 1.0)),
            ("2026-09-20T11:00:00Z", turn("claude-opus-5", 50, 5, 0.5)),
            (
                "2026-09-20T12:00:00Z",
                turn("claude-haiku-4-5", 10, 1, 0.01),
            ),
        ]);

        let (usage, _) = scan_log("a1", &log);
        assert_eq!(usage.turns, 3);
        assert_eq!(usage.last_at, Some(datetime!(2026-09-20 12:00:00 UTC)));
        assert_eq!(usage.models.len(), 2);

        // Costliest model first.
        let opus = &usage.models[0];
        assert_eq!(opus.model, "claude-opus-5");
        assert_eq!(opus.input_tokens, 150);
        assert_eq!(opus.output_tokens, 25);
        assert_eq!(opus.cache_read_tokens, 20);
        assert_eq!(opus.cache_creation_tokens, 10);
        assert_eq!(opus.context_window, Some(200000));
        assert!((opus.cost_usd - 1.5).abs() < 1e-9);
        assert_eq!(usage.models[1].model, "claude-haiku-4-5");
    }

    #[test]
    fn keeps_the_newest_limits_reading() {
        let log = log(&[
            ("2026-09-20T10:00:00Z", limits(0.10, 0.01)),
            ("2026-09-20T12:00:00Z", limits(0.52, 0.04)),
        ]);

        let (_, seen) = scan_log("a1", &log);
        let seen = seen.expect("a limits reading");
        assert_eq!(seen.observed_at, datetime!(2026-09-20 12:00:00 UTC));
        // Shortest window first, whatever order the map came in.
        assert_eq!(seen.windows[0].kind, "five_hour");
        assert_eq!(seen.windows[0].utilization, 0.52);
        assert_eq!(seen.windows[0].resets_at, 1789947600);
        assert_eq!(seen.windows[1].kind, "seven_day");
        assert!(!seen.using_overage);
        assert_eq!(seen.status.as_deref(), Some("allowed"));
    }

    #[test]
    fn ignores_chatter_and_half_written_lines() {
        let mut text = log(&[("2026-09-20T10:00:00Z", json!({ "type": "assistant" }))]);
        text.push_str("{\"ts\":\"2026-09-20T10:00:01Z\",\"event\":{\"modelUsage\"\n");
        text.push_str(&log(&[(
            "2026-09-20T10:00:02Z",
            turn("claude-opus-5", 7, 3, 0.02),
        )]));

        let (usage, seen) = scan_log("a1", &text);
        assert_eq!(usage.turns, 1);
        assert_eq!(usage.models[0].input_tokens, 7);
        assert!(seen.is_none());
    }

    #[test]
    fn summary_is_empty_without_logs() {
        let _env = StateEnv::new();
        let s = summary().unwrap();
        assert!(s.agents.is_empty());
        assert!(s.limits.is_none());
    }

    #[test]
    fn summary_ranks_agents_by_cost_and_takes_the_newest_limits() {
        let _env = StateEnv::new();
        write_log(
            "cheap",
            &log(&[
                (
                    "2026-09-20T10:00:00Z",
                    turn("claude-haiku-4-5", 10, 1, 0.01),
                ),
                ("2026-09-20T13:00:00Z", limits(0.80, 0.09)),
            ]),
        );
        write_log(
            "pricey",
            &log(&[
                ("2026-09-20T11:00:00Z", turn("claude-opus-5", 900, 90, 4.20)),
                ("2026-09-20T12:00:00Z", limits(0.52, 0.04)),
            ]),
        );

        let s = summary().unwrap();
        assert_eq!(s.agents.len(), 2);
        assert_eq!(s.agents[0].agent_id, "pricey");
        assert_eq!(s.agents[1].agent_id, "cheap");

        let seen = s.limits.expect("a limits reading");
        assert_eq!(seen.observed_at, datetime!(2026-09-20 13:00:00 UTC));
        assert_eq!(seen.windows[0].utilization, 0.80);
    }

    #[test]
    fn summary_skips_agents_that_spent_nothing() {
        let _env = StateEnv::new();
        let quiet = log(&[("2026-09-20T10:00:00Z", json!({ "type": "system" }))]);
        write_log("quiet", &quiet);
        assert!(summary().unwrap().agents.is_empty());
    }

    /// Write a raw JSONL log for `agent_id` under the test state dir.
    fn write_log(agent_id: &str, contents: &str) {
        paths::ensure_dirs().unwrap();
        fs::write(paths::agent_log_path(agent_id).unwrap(), contents).unwrap();
    }
}
