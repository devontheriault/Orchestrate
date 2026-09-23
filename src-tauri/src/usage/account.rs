//! Everything Claude Code has spent on this computer, not just the Agents'.
//!
//! Claude Code writes every session it runs to a transcript under
//! `~/.claude/projects` — sessions from a terminal, from an editor, and the
//! ones this app starts for its Agents alike — and each assistant reply there
//! carries its token counts. Totalled, that is the account's spend on this
//! machine. Sessions on other computers or on claude.ai don't land here; the
//! rate-limit windows are the only account-wide reading that covers those.
//!
//! The transcripts run to hundreds of megabytes and the window re-reads every
//! few seconds, so each file's replies are kept after the first read and only
//! a file that has changed since is read again.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::price::{self, Tokens};
use super::{ModelUsage, SlotUsage, SLOT_SECS};

/// Cheap prefilters: only assistant replies carry usage, and parsing every
/// line of every transcript as JSON would cost far more than this scan.
const USAGE_MARKER: &str = "\"usage\"";
const ASSISTANT_MARKER: &str = "\"assistant\"";

/// What Claude Code writes as the model of a reply it made up itself — an
/// error shown as a message, say. No tokens were spent on it.
const SYNTHETIC: &str = "<synthetic>";

/// The account's spend on this computer, every session together.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AccountUsage {
    /// Per Model, biggest spender first.
    pub models: Vec<ModelUsage>,
    /// When the spend happened, oldest slot first. Only slots with spend.
    pub slots: Vec<SlotUsage>,
    /// Replies that ended a Turn — the answers, not the tool calls on the way.
    pub turns: u32,
}

/// One assistant reply as the transcript records it.
#[derive(Debug, Clone, PartialEq)]
struct Reply {
    /// The API's message and request ids together. A reply is written once per
    /// content block, and a resumed session copies its history into the new
    /// transcript, so the same reply turns up many times; this spots it.
    key: Option<String>,
    /// Unix seconds.
    at: i64,
    model: String,
    tokens: Tokens,
    fast: bool,
    /// It ended the Turn rather than asking for a tool.
    answer: bool,
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_creation: Option<CacheCreation>,
    speed: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CacheCreation {
    ephemeral_5m_input_tokens: u64,
    ephemeral_1h_input_tokens: u64,
}

/// A transcript as last read, and how to tell whether it has changed since.
struct Scanned {
    len: u64,
    modified: Option<SystemTime>,
    replies: Vec<Reply>,
}

static CACHE: Mutex<Option<HashMap<PathBuf, Scanned>>> = Mutex::new(None);

/// Total every transcript under `projects`, reading only the ones that
/// changed since the last call.
pub fn summary(projects: &Path) -> AccountUsage {
    let mut files = Vec::new();
    find_transcripts(projects, &mut files);

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let mut old = cache.take().unwrap_or_default();
    let mut fresh = HashMap::with_capacity(files.len());
    for (path, len, modified) in files {
        let kept = old
            .remove(&path)
            .filter(|s| s.len == len && s.modified == modified);
        let scanned = match kept {
            Some(s) => s,
            // A transcript that won't read shouldn't cost the user the rest.
            None => match fs::read_to_string(&path) {
                Ok(text) => Scanned {
                    len,
                    modified,
                    replies: scan(&text),
                },
                Err(_) => continue,
            },
        };
        fresh.insert(path, scanned);
    }

    let usage = total(fresh.values().flat_map(|s| &s.replies));
    // Files that have gone are dropped with `old`.
    *cache = Some(fresh);
    usage
}

/// Every `.jsonl` under `dir`, with its size and modification time. Subagents
/// write theirs a level down, beside the session that ran them.
fn find_transcripts(dir: &Path, out: &mut Vec<(PathBuf, u64, Option<SystemTime>)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() {
            find_transcripts(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push((path, meta.len(), meta.modified().ok()));
        }
    }
}

/// The replies in one transcript, each once — the fullest copy of it, since
/// the lines written before its last content block undercount its output.
fn scan(text: &str) -> Vec<Reply> {
    let mut keyed: HashMap<String, Reply> = HashMap::new();
    let mut unkeyed = Vec::new();
    for line in text.lines() {
        if !line.contains(USAGE_MARKER) || !line.contains(ASSISTANT_MARKER) {
            continue;
        }
        let Some(reply) = parse(line) else { continue };
        match reply.key.clone() {
            Some(key) => match keyed.get(&key) {
                Some(kept) if !fuller(&reply, kept) => {}
                _ => {
                    keyed.insert(key, reply);
                }
            },
            None => unkeyed.push(reply),
        }
    }
    unkeyed.extend(keyed.into_values());
    unkeyed
}

/// Whether `reply` is a later, more complete copy of `kept`.
fn fuller(reply: &Reply, kept: &Reply) -> bool {
    reply.tokens.output > kept.tokens.output || (reply.answer && !kept.answer)
}

fn parse(line: &str) -> Option<Reply> {
    // A half-written last line is normal on a live transcript.
    let line: Line = serde_json::from_str(line).ok()?;
    if line.kind.as_deref() != Some("assistant") {
        return None;
    }
    let message = line.message?;
    let model = message.model?;
    if model == SYNTHETIC {
        return None;
    }
    let usage = message.usage?;
    let at = time::OffsetDateTime::parse(
        line.timestamp.as_deref()?,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()?
    .unix_timestamp();

    // Older transcripts don't split cache writes by lifetime; those were all
    // the API's default five minutes.
    let (write_5m, write_1h) = match &usage.cache_creation {
        Some(c) if c.ephemeral_5m_input_tokens + c.ephemeral_1h_input_tokens > 0 => {
            (c.ephemeral_5m_input_tokens, c.ephemeral_1h_input_tokens)
        }
        _ => (usage.cache_creation_input_tokens, 0),
    };

    Some(Reply {
        key: message
            .id
            .zip(line.request_id)
            .map(|(m, r)| format!("{m}:{r}")),
        at,
        model,
        tokens: Tokens {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_write_5m: write_5m,
            cache_write_1h: write_1h,
        },
        fast: usage.speed.as_deref() == Some("fast"),
        answer: message.stop_reason.is_some_and(|r| r != "tool_use"),
    })
}

/// Add up replies from every transcript, counting a reply that appears in
/// more than one of them once.
fn total<'a>(replies: impl Iterator<Item = &'a Reply>) -> AccountUsage {
    let mut keyed: HashMap<&str, &Reply> = HashMap::new();
    let mut unique = Vec::new();
    for reply in replies {
        match reply.key.as_deref() {
            Some(key) => match keyed.get(key) {
                Some(kept) if !fuller(reply, kept) => {}
                _ => {
                    keyed.insert(key, reply);
                }
            },
            None => unique.push(reply),
        }
    }
    unique.extend(keyed.into_values());

    let mut models: HashMap<String, ModelUsage> = HashMap::new();
    let mut slots: HashMap<i64, SlotUsage> = HashMap::new();
    let mut turns = 0u32;
    for r in unique {
        let cost = price::cost(&r.model, &r.tokens, r.fast);
        let write = r.tokens.cache_write_5m + r.tokens.cache_write_1h;

        let m = models.entry(r.model.clone()).or_insert_with(|| ModelUsage {
            model: r.model.clone(),
            ..ModelUsage::default()
        });
        m.input_tokens += r.tokens.input;
        m.output_tokens += r.tokens.output;
        m.cache_read_tokens += r.tokens.cache_read;
        m.cache_creation_tokens += write;
        m.cost_usd += cost;

        let start = r.at.div_euclid(SLOT_SECS) * SLOT_SECS;
        let s = slots.entry(start).or_insert_with(|| SlotUsage {
            start,
            ..SlotUsage::default()
        });
        s.input_tokens += r.tokens.input;
        s.output_tokens += r.tokens.output;
        s.cache_read_tokens += r.tokens.cache_read;
        s.cache_creation_tokens += write;
        s.cost_usd += cost;
        if r.answer {
            s.turns += 1;
            turns += 1;
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

    AccountUsage {
        models,
        slots,
        turns,
    }
}
