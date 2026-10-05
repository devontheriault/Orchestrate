//! Signing in to a calendar provider with OAuth 2.0 for installed apps, and
//! staying signed in: Google and Microsoft both work this way.
//!
//! The user's browser goes to the provider with a PKCE challenge, and the
//! provider sends it back to a port the Host listens on at 127.0.0.1, so the
//! browser has to be on the Host's machine. The Host then trades the code for
//! tokens, keeps the refresh token, and uses it for a fresh access token
//! whenever the last is about to run out or the provider says it already has.

use std::time::Duration;

use base64::Engine;
use chrono::Utc;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::SyncError;

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
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `application/x-www-form-urlencoded`, for a query or a form body.
pub fn encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `key=value` pairs of a query string, decoded.
pub fn query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
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

fn random(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..len)
        .map(|_| CHARS[rand::random_range(0..CHARS.len())] as char)
        .collect()
}

/// The PKCE challenge for `verifier`: its SHA-256, base64url without padding.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The page the browser lands on when it comes back.
fn page(title: &str, body: &str) -> String {
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <body style=\"font:16px system-ui;margin:4rem auto;max-width:32rem;color:#333\">\
         <h1 style=\"font-size:1.3rem\">{title}</h1><p>{body}</p></body>"
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{html}",
        html.len()
    )
}

/// A sign-in waiting for the browser to come back.
pub struct SignIn {
    /// Where to send the user's browser.
    pub url: String,
    listener: TcpListener,
    /// The same port on IPv6, for a browser that reads `localhost` as `::1`.
    listener6: Option<TcpListener>,
    redirect: String,
    verifier: String,
    state: String,
    oauth: OAuth,
}

impl SignIn {
    /// Listen for the provider's redirect, and say where to send the browser.
    pub async fn start(oauth: &OAuth, client: &Client) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("could not listen for {}'s answer: {e}", oauth.name))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let listener6 = if oauth.loopback == "localhost" {
            TcpListener::bind(("::1", port)).await.ok()
        } else {
            None
        };
        let redirect = format!("http://{}:{port}", oauth.loopback);
        let verifier = random(64);
        let state = random(24);
        let challenge = challenge(&verifier);
        let mut params = vec![
            ("client_id", client.id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", oauth.scope.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
        ];
        params.extend(oauth.extra.iter().copied());
        let url = format!("{}?{}", oauth.auth, encode(&params));
        Ok(Self {
            url,
            listener,
            listener6,
            redirect,
            verifier,
            state,
            oauth: oauth.clone(),
        })
    }

    /// Wait for the browser, and trade its code for tokens.
    pub async fn finish(self, http: &reqwest::Client, client: &Client) -> Result<Tokens, String> {
        let name = self.oauth.name;
        let code = tokio::time::timeout(SIGN_IN_WAIT, self.wait_for_code())
            .await
            .map_err(|_| "the sign-in wasn't finished in the browser in time".to_string())??;
        let mut form = vec![
            ("client_id", client.id.as_str()),
            ("code", code.as_str()),
            ("code_verifier", self.verifier.as_str()),
            ("redirect_uri", self.redirect.as_str()),
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

    async fn accept(&self) -> std::io::Result<TcpStream> {
        match &self.listener6 {
            Some(six) => tokio::select! {
                r = self.listener.accept() => r.map(|(s, _)| s),
                r = six.accept() => r.map(|(s, _)| s),
            },
            None => self.listener.accept().await.map(|(s, _)| s),
        }
    }

    async fn wait_for_code(&self) -> Result<String, String> {
        let name = self.oauth.name;
        loop {
            let mut stream = self.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 16 * 1024];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let target = request
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            if path != "/" {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let params = query_pairs(query);
            let get = |k: &str| {
                params
                    .iter()
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.clone())
            };
            if get("state").as_deref() != Some(self.state.as_str()) {
                // Not the browser this sign-in sent; keep waiting for it.
                let _ = stream
                    .write_all(
                        page(
                            "Not this sign-in",
                            "This page belongs to an older sign-in. You can close it.",
                        )
                        .as_bytes(),
                    )
                    .await;
                continue;
            }
            if let Some(error) = get("error") {
                let _ = stream
                    .write_all(
                        page(
                            &format!("{name} wasn't connected"),
                            "You can close this tab and try again from Orchestrate.",
                        )
                        .as_bytes(),
                    )
                    .await;
                let why = get("error_description").unwrap_or(error);
                return Err(format!("{name} sign-in was not completed ({why})"));
            }
            let Some(code) = get("code") else {
                continue;
            };
            let _ = stream
                .write_all(
                    page(
                        &format!("{name} Calendar is connected"),
                        "You can close this tab and go back to Orchestrate.",
                    )
                    .as_bytes(),
                )
                .await;
            return Ok(code);
        }
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
