//! Signing in to a calendar provider with OAuth 2.0 for installed apps, and
//! staying signed in: Google and Microsoft both work this way.
//!
//! The browser's part is `crate::oauth`'s. The Host then trades the code for
//! tokens, keeps the refresh token, and uses it for a fresh access token
//! whenever the last is about to run out or the provider says it already has.

use std::fmt::Write;
use std::time::Duration;

use chrono::Utc;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SyncError;
use crate::oauth::{Loopback, Pkce};

/// How long a sign-in waits for the browser to come back.
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

/// How a provider signs in.
#[derive(Debug, Clone)]
pub struct OAuth {
    /// The provider's name, for what the user reads: "Google".
    pub name: &'static str,
    pub auth: String,
    pub token: String,
    /// The scopes asked for, space-separated.
    pub scope: String,
    /// Anything else the provider's sign-in page takes.
    pub extra: Vec<(&'static str, &'static str)>,
    /// The host the browser is sent back to. Microsoft only accepts
    /// `localhost` for an app on the user's machine; Google prefers the address.
    pub loopback: &'static str,
    /// Whether the scope goes with a refresh too, as Microsoft wants.
    pub scope_on_refresh: bool,
}

/// The app as the provider knows it: a client ID, and for Google a secret
/// that, for an installed app, is no secret at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

/// What signing in left the Host with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// When `access_token` stops working, in Unix seconds.
    pub expires_at: i64,
}

/// `s` percent-encoded, for a query value or a path segment.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b.into())
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// `application/x-www-form-urlencoded`, for a query or a form body.
pub fn encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn now() -> i64 {
    Utc::now().timestamp()
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

/// Ask the provider's token endpoint for tokens, by the `form` given.
async fn token(
    http: &reqwest::Client,
    oauth: &OAuth,
    form: &[(&str, &str)],
) -> Result<TokenReply, SyncError> {
    let name = oauth.name;
    let res = http
        .post(&oauth.token)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encode(form))
        .send()
        .await
        .map_err(|e| SyncError::Other(format!("could not reach {name}: {e}")))?;
    let status = res.status();
    let body: Value = res.json().await.unwrap_or_default();
    if status.is_success() {
        return serde_json::from_value(body)
            .map_err(|e| SyncError::Other(format!("{name}'s token reply was unreadable: {e}")));
    }
    let code = body["error"].as_str().unwrap_or_default();
    let why = body["error_description"].as_str().unwrap_or(code);
    // A refresh token the user revoked, or one the provider expired (Google
    // does after a week for an app still "in testing"), can't be used again.
    if matches!(
        code,
        "invalid_grant" | "invalid_client" | "unauthorized_client" | "interaction_required"
    ) {
        Err(SyncError::Auth(format!(
            "{name} wants you to sign in again ({why})"
        )))
    } else {
        Err(SyncError::Other(format!(
            "{name} refused the sign-in: {status} {why}"
        )))
    }
}

/// A sign-in waiting for the browser to come back.
pub struct SignIn {
    /// Where to send the user's browser.
    pub url: String,
    loopback: Loopback,
    pkce: Pkce,
    state: String,
    oauth: OAuth,
}

impl SignIn {
    /// Listen for the provider's redirect, and say where to send the browser.
    pub async fn start(oauth: &OAuth, client: &Client) -> Result<Self, String> {
        let loopback = Loopback::bind(oauth.loopback, oauth.name).await?;
        let pkce = Pkce::new();
        let state = crate::oauth::new_state();
        let mut params = vec![
            ("client_id", client.id.as_str()),
            ("redirect_uri", loopback.redirect.as_str()),
            ("response_type", "code"),
            ("scope", oauth.scope.as_str()),
            ("code_challenge", pkce.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
        ];
        params.extend(oauth.extra.iter().copied());
        let url = format!("{}?{}", oauth.auth, encode(&params));
        Ok(Self {
            url,
            loopback,
            pkce,
            state,
            oauth: oauth.clone(),
        })
    }

    /// Wait for the browser, and trade its code for tokens.
    pub async fn finish(self, http: &reqwest::Client, client: &Client) -> Result<Tokens, String> {
        let name = self.oauth.name;
        let done = format!("{name} Calendar is connected");
        let code = tokio::time::timeout(SIGN_IN_WAIT, self.loopback.code(name, &self.state, &done))
            .await
            .map_err(|_| "the sign-in wasn't finished in the browser in time".to_string())??;
        let mut form = vec![
            ("client_id", client.id.as_str()),
            ("code", code.as_str()),
            ("code_verifier", self.pkce.verifier.as_str()),
            ("redirect_uri", self.loopback.redirect.as_str()),
            ("grant_type", "authorization_code"),
        ];
        if let Some(secret) = &client.secret {
            form.push(("client_secret", secret));
        }
        if self.oauth.scope_on_refresh {
            form.push(("scope", &self.oauth.scope));
        }
        let reply = token(http, &self.oauth, &form)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Tokens {
            access_token: reply.access_token,
            refresh_token: reply.refresh_token.ok_or_else(|| {
                format!("{name} gave no refresh token, so the Host couldn't stay signed in")
            })?,
            expires_at: now() + reply.expires_in,
        })
    }
}

/// An answer from a provider's API: its status and its body as JSON (null
/// when it had none).
pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
}

/// One signed-in account, making requests as itself.
pub struct Session<'a> {
    http: &'a reqwest::Client,
    oauth: &'a OAuth,
    client: &'a Client,
    tokens: &'a mut Tokens,
}

impl<'a> Session<'a> {
    pub fn new(
        http: &'a reqwest::Client,
        oauth: &'a OAuth,
        client: &'a Client,
        tokens: &'a mut Tokens,
    ) -> Self {
        Self {
            http,
            oauth,
            client,
            tokens,
        }
    }

    /// Trade the refresh token for a new access token. Microsoft hands back
    /// a new refresh token each time, which replaces the old.
    async fn refresh(&mut self) -> Result<(), SyncError> {
        let mut form = vec![
            ("client_id", self.client.id.as_str()),
            ("refresh_token", self.tokens.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(secret) = &self.client.secret {
            form.push(("client_secret", secret));
        }
        if self.oauth.scope_on_refresh {
            form.push(("scope", &self.oauth.scope));
        }
        let reply = token(self.http, self.oauth, &form).await?;
        self.tokens.access_token = reply.access_token;
        self.tokens.expires_at = now() + reply.expires_in;
        if let Some(r) = reply.refresh_token {
            self.tokens.refresh_token = r;
        }
        Ok(())
    }

    /// A request as the account, refreshing the access token when it's about
    /// to run out, or once when the provider says it already has. Any status
    /// comes back; only reaching the provider at all can fail here.
    pub async fn send(
        &mut self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        headers: &[(&str, &str)],
    ) -> Result<Reply, SyncError> {
        if self.tokens.expires_at - 60 <= now() {
            self.refresh().await?;
        }
        let mut retried = false;
        loop {
            let mut req = self
                .http
                .request(method.clone(), url)
                .bearer_auth(&self.tokens.access_token);
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            if let Some(b) = body {
                req = req.json(b);
            }
            let res = req.send().await.map_err(|e| {
                SyncError::Other(format!("could not reach {}: {e}", self.oauth.name))
            })?;
            let status = res.status();
            if status == StatusCode::UNAUTHORIZED && !retried {
                retried = true;
                self.refresh().await?;
                continue;
            }
            let text = res.text().await.unwrap_or_default();
            let body = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Ok(Reply { status, body });
        }
    }
}
