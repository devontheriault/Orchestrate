//! Which Claude models this user can actually run.
//!
//! The picker is filled from Anthropic's Models API rather than a list baked
//! into the app, so a user sees the models their own account has — including
//! ones released after this build — and never sees one they can't run.
//!
//! We reuse whatever credential Claude Code itself runs on, because an Agent is
//! a `claude` process: asking with a different account than the one that will
//! do the work would offer models the Agent then couldn't use.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const API_VERSION: &str = "2023-06-01";
/// Required alongside a subscription OAuth token; an API key doesn't need it.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// The API's own maximum page size. One page covers every current model, so in
/// practice the pagination loop below runs exactly once.
const PAGE_SIZE: u32 = 100;
/// Bounded so a network that accepts the connection but never answers leaves
/// the picker showing an error rather than "loading…" forever.
const TIMEOUT: Duration = Duration::from_secs(10);

/// One model offered in the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// The full model name, which is what `claude --model` takes.
    pub id: String,
    /// Anthropic's own name for it, e.g. "Claude Opus 4.5".
    pub display_name: String,
}

/// How we prove to the Models API who is asking.
#[derive(Debug, PartialEq, Eq)]
enum Credential {
    /// A plain API key, sent as `x-api-key`.
    ApiKey(String),
    /// A subscription/OAuth token, sent as a bearer plus the OAuth beta flag.
    Bearer(String),
}

/// Where Claude Code keeps the logged-in user's OAuth tokens.
fn claude_credentials_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

fn non_empty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}

/// Find a credential the same way the SDKs and `claude` itself do: an explicit
/// API key wins, then an explicit auth token, then the logged-in session on
/// disk. Returns `None` when the user has never signed in anywhere.
fn find_credential() -> Option<Credential> {
    if let Some(key) = non_empty("ANTHROPIC_API_KEY") {
        return Some(Credential::ApiKey(key));
    }
    if let Some(token) = non_empty("ANTHROPIC_AUTH_TOKEN") {
        return Some(Credential::Bearer(token));
    }
    let path = claude_credentials_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let token = json
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .trim()
        .to_string();
    (!token.is_empty()).then_some(Credential::Bearer(token))
}

/// One page of `GET /v1/models`. Only the fields the picker needs.
#[derive(Debug, Deserialize)]
struct ModelsPage {
    data: Vec<ModelInfo>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

/// The URL for one page, continuing after `after_id` when there is one. Model
/// ids are plain ASCII slugs, so they need no escaping.
fn page_url(after_id: Option<&str>) -> String {
    match after_id {
        Some(id) => format!("{MODELS_URL}?limit={PAGE_SIZE}&after_id={id}"),
        None => format!("{MODELS_URL}?limit={PAGE_SIZE}"),
    }
}

/// Read a page of models out of an API response body. Split out from the
/// request so the shape of what we depend on is testable without a network.
fn parse_page(body: &str) -> Result<ModelsPage> {
    serde_json::from_str(body).map_err(Error::Json)
}

/// Every model the user's account can run, newest first — the order the API
/// returns them in, which is the order a picker wants.
///
/// Fails rather than falling back to a guessed list: the caller can show the
/// reason and offer Claude Code's own default, which always works.
pub async fn list() -> Result<Vec<ModelInfo>> {
    let credential = find_credential().ok_or(Error::NoCredential)?;
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| Error::Models(e.to_string()))?;

    let mut models: Vec<ModelInfo> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut req = client
            .get(page_url(after.as_deref()))
            .header("anthropic-version", API_VERSION);
        req = match &credential {
            Credential::ApiKey(key) => req.header("x-api-key", key),
            Credential::Bearer(token) => req
                .header("authorization", format!("Bearer {token}"))
                .header("anthropic-beta", OAUTH_BETA),
        };

        let response = req.send().await.map_err(|e| Error::Models(e.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| Error::Models(e.to_string()))?;
        if !status.is_success() {
            // The body carries Anthropic's own explanation (expired token, no
            // access); it's far more useful to the user than the status alone.
            return Err(Error::Models(format!("{status}: {}", body.trim())));
        }

        let page = parse_page(&body)?;
        models.extend(page.data);
        match (page.has_more, page.last_id) {
            (true, Some(id)) => after = Some(id),
            // `has_more` without a cursor would loop forever; stop instead.
            _ => break,
        }
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed copy of a real `GET /v1/models` body. The API sends many more
    /// fields per model; we must keep ignoring the ones we don't use.
    const PAGE: &str = r#"{
        "data": [
            {
                "type": "model",
                "id": "claude-opus-4-5-20251101",
                "display_name": "Claude Opus 4.5",
                "created_at": "2025-11-24T00:00:00Z",
                "max_input_tokens": 200000,
                "capabilities": { "batch": { "supported": true } }
            },
            {
                "type": "model",
                "id": "claude-haiku-4-5-20251001",
                "display_name": "Claude Haiku 4.5",
                "created_at": "2025-10-15T00:00:00Z"
            }
        ],
        "has_more": false,
        "first_id": "claude-opus-4-5-20251101",
        "last_id": "claude-haiku-4-5-20251001"
    }"#;

    #[test]
    fn parses_a_models_page_ignoring_unknown_fields() {
        let page = parse_page(PAGE).unwrap();
        assert!(!page.has_more);
        let ids: Vec<&str> = page.data.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["claude-opus-4-5-20251101", "claude-haiku-4-5-20251001"]);
        assert_eq!(page.data[0].display_name, "Claude Opus 4.5");
    }

    /// The version is part of the id we hand `claude --model`, so a picked
    /// model stays the exact one the user picked.
    #[test]
    fn model_ids_keep_their_version_suffix() {
        let page = parse_page(PAGE).unwrap();
        assert!(page.data[0].id.ends_with("-20251101"), "{}", page.data[0].id);
    }

    #[test]
    fn the_first_page_has_no_cursor_and_later_ones_do() {
        assert_eq!(page_url(None), "https://api.anthropic.com/v1/models?limit=100");
        assert_eq!(
            page_url(Some("claude-haiku-4-5-20251001")),
            "https://api.anthropic.com/v1/models?limit=100&after_id=claude-haiku-4-5-20251001"
        );
    }

    #[test]
    fn a_page_without_pagination_fields_is_the_only_page() {
        let page = parse_page(r#"{"data":[]}"#).unwrap();
        assert!(!page.has_more);
        assert_eq!(page.last_id, None);
    }

    /// Guards the env precedence, which decides which account's models we show.
    #[test]
    fn an_api_key_outranks_an_auth_token() {
        temp_env(&[
            ("ANTHROPIC_API_KEY", Some("sk-ant-key")),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-ant-oat")),
        ], || {
            assert_eq!(find_credential(), Some(Credential::ApiKey("sk-ant-key".into())));
        });
    }

    #[test]
    fn an_auth_token_is_sent_as_a_bearer() {
        temp_env(&[
            ("ANTHROPIC_API_KEY", None),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-ant-oat")),
        ], || {
            assert_eq!(find_credential(), Some(Credential::Bearer("sk-ant-oat".into())));
        });
    }

    /// An exported-but-empty key is how a shell profile "unsets" one; treating
    /// it as a credential would send an empty x-api-key and 401.
    #[test]
    fn a_blank_env_var_is_not_a_credential() {
        temp_env(&[
            ("ANTHROPIC_API_KEY", Some("   ")),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-ant-oat")),
        ], || {
            assert_eq!(find_credential(), Some(Credential::Bearer("sk-ant-oat".into())));
        });
    }

    /// Set env vars, run, restore. Serialised because the process environment
    /// is global and these tests would otherwise race each other.
    fn temp_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
    }
}
