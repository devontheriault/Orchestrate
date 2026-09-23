//! What a Model's tokens cost, for the spend Claude Code didn't price itself.
//!
//! An Agent's `result` event carries Claude Code's own `costUSD`, but the
//! session transcripts the account view reads carry only token counts, so
//! those are priced here at Anthropic's list rates. The rates reproduce
//! Claude Code's figure to the cent on the models both have seen; a model this
//! table doesn't know is priced as its family's current model rather than as
//! free, so a new release shows a close number instead of none.

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rates {
    input: f64,
    output: f64,
    cache_read: f64,
}

/// Cache writes are priced off the input rate, by how long the entry lives.
const WRITE_5M: f64 = 1.25;
const WRITE_1H: f64 = 2.0;

/// Fast mode runs the same model at twice the price.
const FAST: f64 = 2.0;

/// Most specific prefix first: the first match wins, so a family's bare name
/// sits after every model in it that is priced differently.
const TABLE: &[(&str, Rates)] = &[
    ("claude-fable-5-1", rates(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", rates(10.0, 50.0, 0.25)),
    ("claude-fable", rates(10.0, 50.0, 1.0)),
    ("claude-mythos", rates(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", rates(4.0, 20.0, 0.2)),
    // Opus 4 and 4.1 predate the price cut that came with 4.5.
    ("claude-opus-4-1", rates(15.0, 75.0, 1.5)),
    ("claude-opus-4-2025", rates(15.0, 75.0, 1.5)),
    ("claude-opus", rates(5.0, 25.0, 0.5)),
    ("claude-sonnet-5", rates(2.0, 10.0, 0.2)),
    ("claude-sonnet", rates(3.0, 15.0, 0.3)),
    ("claude-3-7-sonnet", rates(3.0, 15.0, 0.3)),
    ("claude-3-5-sonnet", rates(3.0, 15.0, 0.3)),
    ("claude-3-5-haiku", rates(0.8, 4.0, 0.08)),
    ("claude-haiku", rates(1.0, 5.0, 0.1)),
];

const fn rates(input: f64, output: f64, cache_read: f64) -> Rates {
    Rates {
        input,
        output,
        cache_read,
    }
}

/// One API reply's tokens, split the way they're billed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
}

/// What `tokens` of `model` cost in USD; zero for a model no row matches.
pub fn cost(model: &str, tokens: &Tokens, fast: bool) -> f64 {
    // Cloud providers prefix the id (`us.anthropic.claude-…`); the rest is ours.
    let model = model.find("claude-").map_or(model, |at| &model[at..]);
    let Some(r) = TABLE
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, r)| r)
    else {
        return 0.0;
    };
    let usd = tokens.input as f64 * r.input
        + tokens.output as f64 * r.output
        + tokens.cache_read as f64 * r.cache_read
        + tokens.cache_write_5m as f64 * r.input * WRITE_5M
        + tokens.cache_write_1h as f64 * r.input * WRITE_1H;
    usd / 1_000_000.0 * if fast { FAST } else { 1.0 }
}
