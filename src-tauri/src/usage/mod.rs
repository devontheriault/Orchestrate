//! What has been spent: tokens and money per Model, and how much of the
//! account's rate-limit windows Claude Code last reported.
//!
//! The Agents' numbers come out of their logs already on disk rather than a
//! counter kept in memory. `claude` reports a Turn's per-model usage on its
//! `result` event and the account's limit windows on `rate_limit_event`, so
//! the logs are the record: reading them means the totals survive a restart,
//! and they cover Agents this window never opened — Reaped ones included,
//! whose logs outlive them. The account's numbers come from Claude Code's own
//! session transcripts instead; see `account`.

use std::collections::HashMap;
use std::fs;

use serde::Serialize;
use serde_json::Value;
use time::OffsetDateTime;

pub use account::AccountUsage;

use crate::domain::AgentEvent;
use crate::error::{Error, Result};
use crate::paths;

mod account;
mod price;

/// Cheap prefilters. Parsing every line of every log as JSON costs far more
/// than scanning for the two shapes that carry usage at all — most lines are
/// assistant text we don't need here.
const RESULT_MARKER: &str = "\"modelUsage\"";
const LIMIT_MARKER: &str = "\"rate_limit_event\"";

/// The order the known limit windows read in: shortest first, the way a user
/// thinks about them. Anything Claude Code adds later sorts after these.
const WINDOW_ORDER: [&str; 3] = ["five_hour", "seven_day", "seven_day_opus"];

/// How finely spend is bucketed in time. Days are the user's, not UTC's, and
/// only the window knows its time zone, so the backend hands over slots small
/// enough to land on the right side of any zone's midnight — the odd ones sit
/// on a quarter hour — and the window groups them into days.
const SLOT_SECS: i64 = 15 * 60;

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

/// What one Agent spent inside one time slot, all Models together.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SlotUsage {
    /// Unix seconds at which the slot opens.
    pub start: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cost_usd: f64,
    /// Turns whose answer landed in the slot.
    pub turns: u32,
}

/// One Agent's share of the total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AgentUsage {
    pub agent_id: String,
    /// Per Model, biggest spender first.
    pub models: Vec<ModelUsage>,
    /// When the spend happened, oldest slot first. Only slots with spend.
    pub slots: Vec<SlotUsage>,
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
    /// Every Claude Code session on this computer, the Agents' among them.
    pub account: AccountUsage,
    /// Agents that have spent anything, costliest first.
    pub agents: Vec<AgentUsage>,
    /// `None` until some Agent has been told its limits — a fresh install, or
    /// an account the API doesn't meter this way.
    pub limits: Option<Limits>,
}

/// Total up what the account spent on this computer, and what each Agent did.
pub fn summary() -> Result<UsageSummary> {
    let account = paths::claude_projects_dir()
        .map(|dir| account::summary(&dir))
        .unwrap_or_default();
    let (agents, limits) = agents()?;
    Ok(UsageSummary {
        account,
        agents,
        limits,
    })
}

/// Read every Agent log: what each spent, and the newest limits reading.
fn agents() -> Result<(Vec<AgentUsage>, Option<Limits>)> {
    let dir = paths::logs_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), None)),
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
    Ok((agents, limits))
}

fn total_cost(a: &AgentUsage) -> f64 {
    a.models.iter().map(|m| m.cost_usd).sum()
}

/// Total one Agent's log: its per-Model spend, plus the newest limits reading
/// it happens to carry.
fn scan_log(agent_id: &str, contents: &str) -> (AgentUsage, Option<Limits>) {
    let mut models: HashMap<String, ModelUsage> = HashMap::new();
    let mut slots: HashMap<i64, SlotUsage> = HashMap::new();
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
                let start = event.ts.unix_timestamp().div_euclid(SLOT_SECS) * SLOT_SECS;
                let slot = slots.entry(start).or_insert_with(|| SlotUsage {
                    start,
                    ..SlotUsage::default()
                });
                fold_slot(slot, usage);
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

    let mut slots: Vec<SlotUsage> = slots.into_values().collect();
    slots.sort_by_key(|s| s.start);

    (
        AgentUsage {
            agent_id: agent_id.to_owned(),
            models,
            slots,
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

/// Add one `result` event's `modelUsage` map to its time slot, Models summed.
fn fold_slot(slot: &mut SlotUsage, usage: &Value) {
    let Some(map) = usage.as_object() else { return };
    for v in map.values() {
        slot.input_tokens += u64_at(v, "inputTokens");
        slot.output_tokens += u64_at(v, "outputTokens");
        slot.cache_read_tokens += u64_at(v, "cacheReadInputTokens");
        slot.cache_creation_tokens += u64_at(v, "cacheCreationInputTokens");
        slot.cost_usd += v.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0);
    }
    slot.turns += 1;
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
mod tests;
