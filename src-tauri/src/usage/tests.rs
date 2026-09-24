use super::price::Tokens;
use super::*;
use crate::test_util::StateEnv;
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;
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
fn buckets_spend_into_time_slots() {
    let log = log(&[
        ("2026-09-20T10:01:00Z", turn("claude-opus-5", 100, 20, 1.0)),
        (
            "2026-09-20T10:14:59Z",
            turn("claude-haiku-4-5", 10, 1, 0.01),
        ),
        ("2026-09-20T10:15:00Z", turn("claude-opus-5", 50, 5, 0.5)),
        ("2026-09-19T23:59:00Z", turn("claude-opus-5", 1, 1, 0.1)),
    ]);

    let (usage, _) = scan_log("a1", &log);
    let starts: Vec<i64> = usage.slots.iter().map(|s| s.start).collect();
    assert_eq!(
        starts,
        vec![
            datetime!(2026-09-19 23:45:00 UTC).unix_timestamp(),
            datetime!(2026-09-20 10:00:00 UTC).unix_timestamp(),
            datetime!(2026-09-20 10:15:00 UTC).unix_timestamp(),
        ]
    );

    // Both models in the 10:00 slot, summed.
    let ten = &usage.slots[1];
    assert_eq!(ten.turns, 2);
    assert_eq!(ten.input_tokens, 110);
    assert_eq!(ten.output_tokens, 21);
    assert_eq!(ten.cache_read_tokens, 20);
    assert_eq!(ten.cache_creation_tokens, 10);
    assert!((ten.cost_usd - 1.01).abs() < 1e-9);
    assert_eq!(usage.slots[2].turns, 1);
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
    assert_eq!(s.account, AccountUsage::default());
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
fn summary_rereads_a_log_that_grew_and_forgets_one_that_went() {
    let _env = StateEnv::new();
    let one = log(&[("2026-09-20T10:00:00Z", turn("claude-opus-5", 100, 10, 1.00))]);
    write_log("growing", &one);
    write_log("gone", &one);
    assert_eq!(summary().unwrap().agents.len(), 2);

    let two = log(&[
        ("2026-09-20T10:00:00Z", turn("claude-opus-5", 100, 10, 1.00)),
        ("2026-09-20T11:00:00Z", turn("claude-opus-5", 200, 20, 2.00)),
    ]);
    write_log("growing", &two);
    fs::remove_file(paths::agent_log_path("gone").unwrap()).unwrap();

    let s = summary().unwrap();
    assert_eq!(s.agents.len(), 1);
    assert_eq!(s.agents[0].turns, 2);
    assert_eq!(s.agents[0].models[0].input_tokens, 300);
}

#[test]
fn summary_skips_agents_that_spent_nothing() {
    let _env = StateEnv::new();
    let quiet = log(&[("2026-09-20T10:00:00Z", json!({ "type": "system" }))]);
    write_log("quiet", &quiet);
    assert!(summary().unwrap().agents.is_empty());
}

/// One assistant line of a Claude Code transcript. Claude Code writes a line
/// per content block, repeating the reply's ids and usage on each.
fn reply(id: &str, ts: &str, model: &str, stop: Option<&str>, input: u64, output: u64) -> Value {
    json!({
        "type": "assistant",
        "timestamp": ts,
        "requestId": format!("req_{id}"),
        "sessionId": "s",
        "message": {
            "id": format!("msg_{id}"),
            "model": model,
            "stop_reason": stop,
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_read_input_tokens": 1000,
                "cache_creation_input_tokens": 200,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": 0,
                    "ephemeral_1h_input_tokens": 200,
                },
                "speed": "standard",
            }
        }
    })
}

fn write_transcript(root: &Path, rel: &str, lines: &[Value]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(path, text).unwrap();
}

#[test]
fn account_counts_each_reply_once_across_every_transcript() {
    let root = TempDir::new().unwrap();
    let opus = "claude-opus-5";
    write_transcript(
        root.path(),
        "-home-me-app/s1.jsonl",
        &[
            json!({ "type": "user", "timestamp": "2026-09-20T10:00:00Z",
                    "message": { "role": "user", "content": "what does \"usage\" mean" } }),
            // The same reply, written as its blocks streamed in: the last line
            // has the full output count and the stop reason.
            reply("a", "2026-09-20T10:00:01Z", opus, None, 10, 4),
            reply("a", "2026-09-20T10:00:02Z", opus, Some("tool_use"), 10, 100),
            reply("b", "2026-09-20T10:00:05Z", opus, Some("end_turn"), 20, 50),
            json!({ "type": "assistant", "timestamp": "2026-09-20T10:00:06Z",
                    "message": { "id": "x", "model": "<synthetic>", "stop_reason": "stop_sequence",
                                 "usage": { "input_tokens": 0, "output_tokens": 0 } } }),
        ],
    );
    // A resumed session carries its history into a new transcript.
    write_transcript(
        root.path(),
        "-home-me-app/s2.jsonl",
        &[
            reply("b", "2026-09-20T10:00:05Z", opus, Some("end_turn"), 20, 50),
            reply(
                "c",
                "2026-09-21T09:00:00Z",
                "claude-sonnet-5",
                Some("end_turn"),
                5,
                5,
            ),
        ],
    );
    // Subagents write theirs a level down, beside the session.
    write_transcript(
        root.path(),
        "-home-me-other/s3/subagents/agent-1.jsonl",
        &[reply(
            "d",
            "2026-09-21T09:30:00Z",
            "claude-haiku-4-5-20251001",
            Some("end_turn"),
            1,
            1,
        )],
    );
    let half = root.path().join("-home-me-other/s3.jsonl");
    fs::write(&half, "{\"type\":\"assistant\",\"message\":{\"usage\"").unwrap();

    let usage = account::summary(root.path());
    assert_eq!(usage.turns, 3);
    let names: Vec<&str> = usage.models.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(
        names,
        vec![opus, "claude-sonnet-5", "claude-haiku-4-5-20251001"]
    );

    let o = &usage.models[0];
    assert_eq!(o.input_tokens, 30);
    assert_eq!(o.output_tokens, 150);
    assert_eq!(o.cache_read_tokens, 2000);
    assert_eq!(o.cache_creation_tokens, 400);
    // 30 in × $5, 150 out × $25, 2000 read × $0.50, 400 one-hour writes × $10.
    let want = (30.0 * 5.0 + 150.0 * 25.0 + 2000.0 * 0.5 + 400.0 * 10.0) / 1e6;
    assert!(
        (o.cost_usd - want).abs() < 1e-12,
        "{} vs {want}",
        o.cost_usd
    );

    let starts: Vec<i64> = usage.slots.iter().map(|s| s.start).collect();
    assert_eq!(
        starts,
        vec![
            datetime!(2026-09-20 10:00:00 UTC).unix_timestamp(),
            datetime!(2026-09-21 09:00:00 UTC).unix_timestamp(),
            datetime!(2026-09-21 09:30:00 UTC).unix_timestamp(),
        ]
    );
    assert_eq!(usage.slots[0].turns, 1);
    assert_eq!(usage.slots[0].output_tokens, 150);
}

#[test]
fn account_rereads_a_transcript_that_grew() {
    let root = TempDir::new().unwrap();
    let first = reply(
        "a",
        "2026-09-20T10:00:00Z",
        "claude-opus-5",
        Some("end_turn"),
        1,
        1,
    );
    write_transcript(root.path(), "p/s.jsonl", &[first.clone()]);
    assert_eq!(account::summary(root.path()).turns, 1);

    let second = reply(
        "b",
        "2026-09-20T10:05:00Z",
        "claude-opus-5",
        Some("end_turn"),
        1,
        1,
    );
    write_transcript(root.path(), "p/s.jsonl", &[first, second]);
    assert_eq!(account::summary(root.path()).turns, 2);

    fs::remove_file(root.path().join("p/s.jsonl")).unwrap();
    assert_eq!(account::summary(root.path()), AccountUsage::default());
}

#[test]
fn prices_models_at_list_rates() {
    let t = Tokens {
        input: 1_000_000,
        output: 1_000_000,
        cache_read: 1_000_000,
        cache_write_5m: 1_000_000,
        cache_write_1h: 1_000_000,
    };
    let at = |model: &str, fast: bool| price::cost(model, &t, fast);
    // in + out + read + 1.25 × in + 2 × in
    assert!((at("claude-opus-5", false) - (5.0 + 25.0 + 0.5 + 6.25 + 10.0)).abs() < 1e-9);
    assert!((at("claude-opus-5-5", false) - (4.0 + 20.0 + 0.2 + 5.0 + 8.0)).abs() < 1e-9);
    assert!((at("claude-fable-5-1", false) - (10.0 + 50.0 + 0.25 + 12.5 + 20.0)).abs() < 1e-9);
    assert!(
        (at("claude-opus-4-20250514", false) - (15.0 + 75.0 + 1.5 + 18.75 + 30.0)).abs() < 1e-9
    );
    assert_eq!(at("claude-opus-5", true), 2.0 * at("claude-opus-5", false));
    // A cloud provider's id prices as the model it names.
    assert_eq!(
        at("us.anthropic.claude-sonnet-5", false),
        at("claude-sonnet-5", false)
    );
    // A release this table predates prices as its family, not as free.
    assert_eq!(at("claude-haiku-5", false), at("claude-haiku-4-5", false));
    assert_eq!(at("gpt-5", false), 0.0);
}

#[test]
fn summary_reads_the_account_from_claude_codes_config_dir() {
    let _env = StateEnv::new();
    let projects = paths::claude_projects_dir().unwrap();
    write_transcript(
        &projects,
        "p/s.jsonl",
        &[reply(
            "a",
            "2026-09-20T10:00:00Z",
            "claude-opus-5",
            Some("end_turn"),
            1,
            1,
        )],
    );
    let s = summary().unwrap();
    assert_eq!(s.account.turns, 1);
    assert!(s.agents.is_empty());
}

/// Write a raw JSONL log for `agent_id` under the test state dir.
fn write_log(agent_id: &str, contents: &str) {
    paths::ensure_dirs().unwrap();
    fs::write(paths::agent_log_path(agent_id).unwrap(), contents).unwrap();
}
