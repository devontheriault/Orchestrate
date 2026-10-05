//! The browser half of signing in with OAuth 2.0 for installed apps, which the
//! Calendar and Mail Spaces both do with Google and Microsoft (ADR 0017).
//!
//! The user's browser goes to the provider with a PKCE challenge, and the
//! provider sends it back to a one-off listener on this machine's loopback
//! address with a code, so the browser has to be on the Host's machine. What
//! the code is then traded for, and how the tokens are kept, is each Space's.

use base64::Engine;
use rand::distr::{Alphanumeric, SampleString};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A PKCE verifier, kept until the code is traded, and the challenge the
/// sign-in page is sent with it.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = Alphanumeric.sample_string(&mut rand::rng(), 64);
        let challenge = challenge(&verifier);
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// The PKCE challenge for `verifier`: its SHA-256, base64url without padding.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh `state`, which tells this sign-in's answer from an older one's.
pub fn new_state() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

/// Where the provider sends the browser back to: a port on the loopback
/// address, listened on until the sign-in's answer arrives.
pub struct Loopback {
    /// The `redirect_uri` the provider is given.
    pub redirect: String,
    v4: TcpListener,
    /// The same port on IPv6, for a browser that reads `localhost` as `::1`.
    v6: Option<TcpListener>,
}

impl Loopback {
    /// Listen on a free port. `host` is the name the redirect uses:
    /// Microsoft's desktop clients are registered with `localhost`, and
    /// match it on any port; Google's take the address. `who` names the
    /// provider in an error.
    pub async fn bind(host: &str, who: &str) -> Result<Self, String> {
        let v4 = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("could not listen for {who}'s answer: {e}"))?;
        let port = v4.local_addr().map_err(|e| e.to_string())?.port();
        let v6 = if host == "localhost" {
            TcpListener::bind(("::1", port)).await.ok()
        } else {
            None
        };
        Ok(Self {
            redirect: format!("http://{host}:{port}"),
            v4,
            v6,
        })
    }

    async fn accept(&self) -> std::io::Result<TcpStream> {
        match &self.v6 {
            Some(v6) => tokio::select! {
                r = self.v4.accept() => r.map(|(s, _)| s),
                r = v6.accept() => r.map(|(s, _)| s),
            },
            None => self.v4.accept().await.map(|(s, _)| s),
        }
    }

    /// Wait for the browser to come back with the code for the sign-in sent
    /// out with `state`, and tell the user how it went: `done` is the title
    /// of the page they land on when it worked. A stray request, or one from
    /// an older sign-in, is answered and waited past.
    pub async fn code(&self, who: &str, state: &str, done: &str) -> Result<String, String> {
        loop {
            let mut stream = self
                .accept()
                .await
                .map_err(|e| format!("lost the {who} sign-in: {e}"))?;
            let mut buf = vec![0u8; 16 * 1024];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let Some(query) = redirect_query(&request) else {
                // A browser asking for the favicon, say.
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            };
            let got = |k: &str| {
                url::form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.into_owned())
            };
            if got("state").as_deref() != Some(state) {
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
            if let Some(code) = got("code") {
                let _ = stream
                    .write_all(
                        page(done, "You can close this tab and go back to Orchestrate.").as_bytes(),
                    )
                    .await;
                return Ok(code);
            }
            let _ = stream
                .write_all(
                    page(
                        &format!("{who} wasn't connected"),
                        "You can close this tab and try again from Orchestrate.",
                    )
                    .as_bytes(),
                )
                .await;
            return Err(match (got("error").as_deref(), got("error_description")) {
                (Some("access_denied"), _) => format!("the {who} sign-in was cancelled"),
                (Some(e), Some(why)) => format!("{who} refused the sign-in: {e}: {why}"),
                (Some(e), None) => format!("{who} refused the sign-in: {e}"),
                (None, _) => format!("{who}'s answer had no sign-in code"),
            });
        }
    }
}

/// The query of a request for the redirect's own path, `/`: empty when it
/// has none, and nothing for any other path or method.
pub fn redirect_query(request: &str) -> Option<&str> {
    let target = request
        .lines()
        .next()?
        .strip_prefix("GET ")?
        .split(' ')
        .next()?;
    match target.split_once('?') {
        Some(("/", query)) => Some(query),
        None if target == "/" => Some(""),
        _ => None,
    }
}

/// A whole HTTP response holding the page the browser lands on.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_is_only_a_get_of_its_own_path() {
        assert_eq!(
            redirect_query("GET /?state=s&code=4%2F0A HTTP/1.1\r\nHost: 127.0.0.1\r\n"),
            Some("state=s&code=4%2F0A")
        );
        assert_eq!(redirect_query("GET / HTTP/1.1\r\n"), Some(""));
        assert_eq!(redirect_query("GET /favicon.ico HTTP/1.1\r\n"), None);
        assert_eq!(redirect_query("GET /x?code=y HTTP/1.1\r\n"), None);
        assert_eq!(redirect_query("POST /?code=x HTTP/1.1\r\n"), None);
    }

    #[test]
    fn a_challenge_is_the_verifiers_sha256() {
        // RFC 7636's own example.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = Pkce::new();
        assert_eq!(p.challenge, challenge(&p.verifier));
        assert_ne!(new_state(), new_state());
    }
}
