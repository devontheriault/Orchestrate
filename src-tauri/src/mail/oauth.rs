//! Signing in with Google or Microsoft, for IMAP's XOAUTH2. The flow is the
//! one both give installed apps: the user signs in in their own browser, which
//! comes back to a one-off listener on this machine's loopback address with a
//! code, and the code (with its PKCE proof) buys a refresh token.
//!
//! Gmail takes an app password too, but Microsoft no longer lets other apps
//! sign in to Outlook.com or Microsoft 365 with a password at all, so for
//! Outlook this is the only way in.
//!
//! The OAuth client is the user's own: made in Google's Cloud console, or
//! registered in Microsoft's Entra admin center. Full mail access is a
//! "restricted" scope at Google, which an unverified app may use only for its
//! owner's own accounts (ADR 0017). The refresh token is kept with the account
//! in its private file, and access tokens only in memory.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use rand::distr::{Alphanumeric, SampleString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::account::{self, OAuthProvider, OAuthSetup, Saved, Secret};
use super::Result;

/// How long the user has to finish signing in.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Where a provider signs people in, and what it is asked for.
#[derive(Debug, PartialEq)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    /// IMAP access, a refresh token, and the address the account belongs to,
    /// so the account saved is the one actually signed in to.
    pub scope: &'static str,
    /// The loopback name the browser is sent back to. Microsoft's desktop
    /// clients are registered with `http://localhost`, and match it on any
    /// port; Google's take the address.
    pub loopback: &'static str,
}

const MICROSOFT_SCOPE: &str =
    "https://outlook.office.com/IMAP.AccessAsUser.All offline_access openid email profile";

pub fn endpoints(provider: OAuthProvider, tenant: Option<&str>) -> Result<Endpoints> {
    Ok(match provider {
        OAuthProvider::Google => Endpoints {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            scope: "https://mail.google.com/ openid email",
            loopback: "127.0.0.1",
        },
        OAuthProvider::Microsoft => {
            let tenant = tenant
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or("common");
            // It goes into the URL's path, so it may only be a directory's
            // name or id.
            if !tenant
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
            {
                return Err(format!("{tenant} isn't a Microsoft directory name or id"));
            }
            let base = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0");
            Endpoints {
                auth: format!("{base}/authorize"),
                token: format!("{base}/token"),
                scope: MICROSOFT_SCOPE,
                loopback: "localhost",
            }
        }
    })
}

/// The page the user signs in on.
pub fn auth_url(
    provider: OAuthProvider,
    setup: &OAuthSetup,
    redirect: &str,
    challenge: &str,
    state: &str,
) -> Result<String> {
    let ends = endpoints(provider, setup.tenant.as_deref())?;
    let mut params = vec![
        ("client_id", setup.client_id.trim()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", ends.scope),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ];
    match provider {
        OAuthProvider::Google => {
            params.push(("access_type", "offline"));
            // Asked every time, so Google hands out a refresh token every time.
            params.push(("prompt", "consent"));
        }
        OAuthProvider::Microsoft => params.push(("prompt", "select_account")),
    }
    let hint = setup.email.trim();
    if !hint.is_empty() {
        params.push(("login_hint", hint));
    }
    url::Url::parse_with_params(&ends.auth, &params)
        .map(|u| u.to_string())
        .map_err(|e| e.to_string())
}

/// The token endpoint's form for `grant`: the code with its proof, or a
/// refresh token. Microsoft's desktop clients have no secret to send, and
/// Microsoft wants the scope named again.
pub fn token_form(
    provider: OAuthProvider,
    client_id: &str,
    client_secret: Option<&str>,
    grant: Grant<'_>,
) -> Vec<(&'static str, String)> {
    let mut form = vec![("client_id", client_id.trim().to_string())];
    if let Some(secret) = client_secret.map(str::trim).filter(|s| !s.is_empty()) {
        form.push(("client_secret", secret.to_string()));
    }
    match grant {
        Grant::Code {
            code,
            verifier,
            redirect,
        } => form.extend([
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("code_verifier", verifier.to_string()),
            ("redirect_uri", redirect.to_string()),
        ]),
        Grant::Refresh(token) => form.extend([
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", token.to_string()),
        ]),
    }
    if provider == OAuthProvider::Microsoft {
        form.push(("scope", MICROSOFT_SCOPE.to_string()));
    }
    form
}

pub enum Grant<'a> {
    Code {
        code: &'a str,
        verifier: &'a str,
        redirect: &'a str,
    },
    Refresh(&'a str),
}

/// Start a sign-in: listen for the provider's answer, and return the page the
/// user signs in on. `done` is called once, with the account or why there
/// isn't one.
pub async fn begin<F, Fut>(provider: OAuthProvider, setup: OAuthSetup, done: F) -> Result<String>
where
    F: FnOnce(Result<Saved>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let who = provider.name();
    if setup.client_id.trim().is_empty() {
        return Err(format!("{who} sign-in needs your app's client ID"));
    }
    if provider == OAuthProvider::Google
        && setup
            .client_secret
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
    {
        return Err("Google sign-in needs your OAuth client's secret too".into());
    }
    let ends = endpoints(provider, setup.tenant.as_deref())?;
    let v4 = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("could not listen for {who}'s answer: {e}"))?;
    let port = v4.local_addr().map_err(|e| e.to_string())?.port();
    // `localhost` may reach us over IPv6 first.
    let v6 = if ends.loopback == "localhost" {
        TcpListener::bind(("::1", port)).await.ok()
    } else {
        None
    };
    let redirect = format!("http://{}:{port}", ends.loopback);

    let verifier = Alphanumeric.sample_string(&mut rand::rng(), 64);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let state = Alphanumeric.sample_string(&mut rand::rng(), 32);
    let url = auth_url(provider, &setup, &redirect, &challenge, &state)?;

    tokio::spawn(async move {
        let answered = answer(provider, &v4, v6.as_ref(), &state);
        let outcome = match tokio::time::timeout(SIGN_IN_WINDOW, answered).await {
            Ok(Ok(code)) => exchange(provider, &setup, &code, &verifier, &redirect).await,
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!("the {who} sign-in wasn't finished in time")),
        };
        done(outcome).await;
    });
    Ok(url)
}

async fn accept(v4: &TcpListener, v6: Option<&TcpListener>) -> std::io::Result<TcpStream> {
    match v6 {
        Some(v6) => tokio::select! {
            r = v4.accept() => r.map(|(s, _)| s),
            r = v6.accept() => r.map(|(s, _)| s),
        },
        None => v4.accept().await.map(|(s, _)| s),
    }
}

/// Wait for the browser to come back with a code for this sign-in.
async fn answer(
    provider: OAuthProvider,
    v4: &TcpListener,
    v6: Option<&TcpListener>,
    state: &str,
) -> Result<String> {
    let who = provider.name();
    loop {
        let mut stream = accept(v4, v6)
            .await
            .map_err(|e| format!("lost the {who} sign-in: {e}"))?;
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
            Err(format!("the answer from {who} didn't match this sign-in"))
        } else if let Some(code) = got("code") {
            Ok(code)
        } else {
            Err(match (got("error").as_deref(), got("error_description")) {
                (Some("access_denied"), _) => format!("the {who} sign-in was cancelled"),
                (Some(e), Some(why)) => format!("{who} refused the sign-in: {e}: {why}"),
                (Some(e), None) => format!("{who} refused the sign-in: {e}"),
                (None, _) => format!("{who}'s answer had no sign-in code"),
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

/// What the browser shows once the provider has sent it back.
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

async fn post(provider: OAuthProvider, token_url: &str, form: &[(&str, String)]) -> Result<Tokens> {
    let who = provider.name();
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = reqwest::Client::new()
        .post(token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("can't reach {who}: {e}"))?;
    let status = response.status();
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    if status.is_success() {
        return serde_json::from_slice(&body)
            .map_err(|e| format!("{who}'s answer made no sense: {e}"));
    }
    Err(match serde_json::from_slice::<TokenError>(&body) {
        Ok(e) if e.error == "invalid_grant" => {
            format!("{who} no longer lets this app into the account; sign in again")
        }
        Ok(e) => format!("{who} refused: {}", e.error_description.unwrap_or(e.error)),
        Err(_) => format!("{who} refused, with HTTP {status}"),
    })
}

async fn exchange(
    provider: OAuthProvider,
    setup: &OAuthSetup,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<Saved> {
    let ends = endpoints(provider, setup.tenant.as_deref())?;
    let form = token_form(
        provider,
        &setup.client_id,
        setup.client_secret.as_deref(),
        Grant::Code {
            code,
            verifier,
            redirect,
        },
    );
    let tokens = post(provider, &ends.token, &form).await?;
    let refresh = tokens.refresh_token.clone().ok_or(match provider {
        OAuthProvider::Google => {
            "Google gave no refresh token. Remove the app at myaccount.google.com/permissions \
             and sign in again"
        }
        OAuthProvider::Microsoft => {
            "Microsoft gave no refresh token. Check the app may ask for offline_access, \
             and sign in again"
        }
    })?;
    let email = tokens
        .id_token
        .as_deref()
        .and_then(email_of)
        .unwrap_or_else(|| setup.email.trim().to_string());
    if email.is_empty() {
        return Err(format!(
            "{} didn't say which account was signed in to",
            provider.name()
        ));
    }
    let saved = Saved::oauth(provider, email, setup, refresh);
    remember(&saved, &tokens);
    Ok(saved)
}

/// The address in an ID token: Google's `email`, or Microsoft's
/// `preferred_username` for an account that keeps its address there. Read
/// without checking the signature: it came straight from the provider's token
/// endpoint, over TLS, in answer to our own code.
pub fn email_of(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    #[derive(Deserialize)]
    struct Claims {
        email: Option<String>,
        preferred_username: Option<String>,
    }
    let claims = serde_json::from_slice::<Claims>(&bytes).ok()?;
    claims
        .email
        .or(claims.preferred_username)
        .filter(|e| e.contains('@'))
}

/// What a signed-in account was last handed: its access token, when that
/// runs out, and the newest refresh token, which Microsoft replaces as it goes.
struct Held {
    access: String,
    expires: Instant,
    refresh: String,
}

static ACCESS: Mutex<Option<HashMap<String, Held>>> = Mutex::new(None);

fn key(saved: &Saved) -> String {
    match &saved.secret {
        Secret::OAuth {
            provider,
            client_id,
            ..
        } => format!("{provider:?}:{client_id}:{}", saved.email),
        Secret::Password { .. } => saved.email.clone(),
    }
}

fn remember(saved: &Saved, tokens: &Tokens) {
    let Secret::OAuth { refresh_token, .. } = &saved.secret else {
        return;
    };
    let held = Held {
        access: tokens.access_token.clone(),
        expires: Instant::now() + Duration::from_secs(tokens.expires_in),
        refresh: tokens
            .refresh_token
            .clone()
            .unwrap_or_else(|| refresh_token.clone()),
    };
    ACCESS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(key(saved), held);
}

/// Hand an access token to an account without signing in, for tests against
/// a local server that takes any token.
#[cfg(test)]
pub fn hold(saved: &Saved, access: &str) {
    remember(
        saved,
        &Tokens {
            access_token: access.into(),
            expires_in: 3600,
            refresh_token: None,
            id_token: None,
        },
    );
}

/// An access token for the account, refreshed when the last is near its end.
/// A refresh token the provider replaces is saved with the account, so it
/// still signs in after the Host restarts.
pub async fn access_token(saved: &Saved) -> Result<String> {
    let Secret::OAuth {
        provider,
        client_id,
        client_secret,
        tenant,
        refresh_token,
    } = &saved.secret
    else {
        return Err("this account signs in with a password".into());
    };
    let refresh = {
        let held = ACCESS.lock().unwrap_or_else(|e| e.into_inner());
        match held.as_ref().and_then(|m| m.get(&key(saved))) {
            Some(h) if h.expires > Instant::now() + Duration::from_secs(60) => {
                return Ok(h.access.clone());
            }
            Some(h) => h.refresh.clone(),
            None => refresh_token.clone(),
        }
    };
    let ends = endpoints(*provider, tenant.as_deref())?;
    let form = token_form(
        *provider,
        client_id,
        client_secret.as_deref(),
        Grant::Refresh(&refresh),
    );
    let tokens = post(*provider, &ends.token, &form).await?;
    remember(saved, &tokens);
    if let Some(rotated) = tokens.refresh_token.as_deref().filter(|r| *r != refresh) {
        keep_refresh_token(saved, rotated);
    }
    Ok(tokens.access_token)
}

/// Save a replaced refresh token over the old one, if the account on disk is
/// still the one it was handed for.
fn keep_refresh_token(saved: &Saved, rotated: &str) {
    let Ok(Some(mut on_disk)) = account::load() else {
        return;
    };
    if key(&on_disk) != key(saved) {
        return;
    }
    if let Secret::OAuth { refresh_token, .. } = &mut on_disk.secret {
        *refresh_token = rotated.to_string();
    }
    if let Err(e) = account::save(&on_disk) {
        eprintln!("mail: could not keep the new refresh token: {e}");
    }
}
