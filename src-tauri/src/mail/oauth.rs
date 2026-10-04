//! Signing in to Gmail with Google, for IMAP's XOAUTH2. The flow is the one
//! Google gives installed apps: the user signs in in their own browser, which
//! comes back to a one-off listener on this machine's loopback address with a
//! code, and the code (with its PKCE proof) buys a refresh token.
//!
//! The OAuth client is the user's own, made in their Google Cloud console:
//! full mail access is a "restricted" scope, which Google lets an unverified
//! app use only for its owner's own accounts (ADR 0017). The refresh token is
//! kept with the account, in its private file; access tokens only in memory.

use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use rand::distr::{Alphanumeric, SampleString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::account::{GoogleSetup, Saved, Secret};
use super::Result;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Full IMAP access, which is what Gmail's XOAUTH2 asks for, and the address
/// it belongs to, so the account is the one actually signed in to.
const SCOPE: &str = "https://mail.google.com/ openid email";

/// How long the user has to finish signing in.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Start a sign-in: listen for Google's answer, and return the page the user
/// signs in on. `done` is called once, with the account or why there isn't one.
pub async fn begin<F, Fut>(g: GoogleSetup, done: F) -> Result<String>
where
    F: FnOnce(Result<Saved>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let client_id = g.client_id.trim().to_string();
    let client_secret = g.client_secret.trim().to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        return Err("Google sign-in needs your OAuth client's ID and secret".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("could not listen for Google's answer: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");

    let verifier = Alphanumeric.sample_string(&mut rand::rng(), 64);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let state = Alphanumeric.sample_string(&mut rand::rng(), 32);

    let mut params = vec![
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        // Asked every time, so Google hands out a refresh token every time.
        ("prompt", "consent"),
        ("state", state.as_str()),
    ];
    let hint = g.email.trim().to_string();
    if !hint.is_empty() {
        params.push(("login_hint", hint.as_str()));
    }
    let url = url::Url::parse_with_params(AUTH_URL, &params)
        .map_err(|e| e.to_string())?
        .to_string();

    tokio::spawn(async move {
        let outcome = match tokio::time::timeout(SIGN_IN_WINDOW, answer(&listener, &state)).await {
            Ok(Ok(code)) => {
                exchange(
                    &code,
                    &verifier,
                    &redirect,
                    &client_id,
                    &client_secret,
                    &hint,
                )
                .await
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the Google sign-in wasn't finished in time".into()),
        };
        done(outcome).await;
    });
    Ok(url)
}

/// Wait for the browser to come back with a code for this sign-in.
async fn answer(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| format!("lost the Google sign-in: {e}"))?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let Some(query) = redirect_query(&request) else {
            // A browser asking for the favicon, say.
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let got = |k: &str| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        let outcome = if got("state").as_deref() != Some(state) {
            Err("the answer from Google didn't match this sign-in".to_string())
        } else if let Some(code) = got("code") {
            Ok(code)
        } else {
            Err(match got("error").as_deref() {
                Some("access_denied") => "the Google sign-in was cancelled".to_string(),
                Some(e) => format!("Google refused the sign-in: {e}"),
                None => "Google's answer had no sign-in code".to_string(),
            })
        };
        let page = page(outcome.as_ref().err().map(String::as_str));
        let _ = stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                )
                .as_bytes(),
            )
            .await;
        return outcome;
    }
}

/// The query of a request for the redirect's own path, `/`.
pub fn redirect_query(request: &str) -> Option<&str> {
    let target = request
        .lines()
        .next()?
        .strip_prefix("GET ")?
        .split(' ')
        .next()?;
    target.strip_prefix("/?")
}

/// What the browser shows once Google has sent it back.
fn page(error: Option<&str>) -> String {
    let (title, line) = match error {
        None => (
            "Signed in",
            "You can close this tab and go back to Orchestrate.",
        ),
        Some(_) => ("Not signed in", "Go back to Orchestrate to try again."),
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;\
         color:#333\"><div><h2>{title}</h2><p>{line}</p></div>"
    )
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

async fn post(form: &[(&str, &str)]) -> Result<Tokens> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("can't reach Google: {e}"))?;
    let status = response.status();
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    if status.is_success() {
        return serde_json::from_slice(&body)
            .map_err(|e| format!("Google's answer made no sense: {e}"));
    }
    Err(match serde_json::from_slice::<TokenError>(&body) {
        Ok(e) if e.error == "invalid_grant" => {
            "Google no longer lets this app into the account; sign in again".to_string()
        }
        Ok(e) => format!("Google refused: {}", e.error_description.unwrap_or(e.error)),
        Err(_) => format!("Google refused, with HTTP {status}"),
    })
}

async fn exchange(
    code: &str,
    verifier: &str,
    redirect: &str,
    client_id: &str,
    client_secret: &str,
    hint: &str,
) -> Result<Saved> {
    let tokens = post(&[
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "authorization_code"),
    ])
    .await?;
    let refresh = tokens.refresh_token.clone().ok_or(
        "Google gave no refresh token. Remove the app at myaccount.google.com/permissions \
         and sign in again",
    )?;
    let email = tokens
        .id_token
        .as_deref()
        .and_then(email_of)
        .unwrap_or_else(|| hint.to_string());
    if email.is_empty() {
        return Err("Google didn't say which account was signed in to".into());
    }
    remember(&refresh, &tokens);
    Ok(Saved::google(
        email,
        client_id.into(),
        client_secret.into(),
        refresh,
    ))
}

/// The address in an ID token. Read without checking its signature: it came
/// straight from Google's token endpoint, over TLS, in answer to our own code.
pub fn email_of(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    #[derive(Deserialize)]
    struct Claims {
        email: Option<String>,
    }
    serde_json::from_slice::<Claims>(&bytes).ok()?.email
}

/// The access token last handed out, by the refresh token it came from.
static ACCESS: Mutex<Option<(String, String, Instant)>> = Mutex::new(None);

fn remember(refresh: &str, tokens: &Tokens) {
    let expires = Instant::now() + Duration::from_secs(tokens.expires_in);
    *ACCESS.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((refresh.to_string(), tokens.access_token.clone(), expires));
}

/// An access token for the account, refreshed when the last is near its end.
pub async fn access_token(saved: &Saved) -> Result<String> {
    let Secret::Google {
        client_id,
        client_secret,
        refresh_token,
    } = &saved.secret
    else {
        return Err("this account doesn't sign in with Google".into());
    };
    if let Some((r, token, expires)) = &*ACCESS.lock().unwrap_or_else(|e| e.into_inner()) {
        if r == refresh_token && *expires > Instant::now() + Duration::from_secs(60) {
            return Ok(token.clone());
        }
    }
    let tokens = post(&[
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "refresh_token"),
    ])
    .await?;
    remember(refresh_token, &tokens);
    Ok(tokens.access_token)
}
