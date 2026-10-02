//! Browser-approved login, `gh auth login` style, with no Real
//! Geeks password or client secret in the CLI:
//!
//! 1. `POST /api/v3/auth/cli/start/` registers the key name, scopes, the
//!    loopback port we listen on, a random `state` and a PKCE-style S256
//!    challenge. The site answers with an admin URL.
//! 2. The browser opens that URL. The site's own admin login runs (with
//!    2FA), then the owner approves or denies. Approve sends the browser to
//!    `http://127.0.0.1:<port>/callback?code=…&state=…`.
//! 3. `POST /api/v3/auth/cli/token/` swaps the one-time code plus our
//!    verifier for the key.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::Rng as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::client::Client;
use crate::error::{Error, Result};

pub const START_PATH: &str = "auth/cli/start/";
pub const TOKEN_PATH: &str = "auth/cli/token/";

/// PKCE-style verifier/challenge pair. The verifier is 32 random bytes as
/// base64url (43 chars), the challenge its SHA-256 in the same encoding.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        Self::from_verifier(&URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn from_verifier(verifier: &str) -> Self {
        let digest = Sha256::digest(verifier.as_bytes());
        Self {
            verifier: verifier.to_string(),
            challenge: URL_SAFE_NO_PAD.encode(digest),
        }
    }
}

pub fn random_state() -> String {
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// What the CLI asks the site for.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub name: String,
    pub scopes: Vec<String>,
    /// Fixed loopback port; `None` lets the OS pick one.
    pub port: Option<u16>,
    /// Caller-supplied state; `None` generates a random one.
    pub state: Option<String>,
}

/// A registered request waiting for the browser.
#[derive(Debug)]
pub struct Pending {
    listener: TcpListener,
    pub state: String,
    pub pkce: Pkce,
    pub request_id: String,
    pub authorize_url: String,
    pub expires_in: u64,
}

impl Pending {
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or_default()
    }

    /// Block until the browser hits the loopback callback, then redeem the
    /// code. Returns the site's key response (`api_key`, `key`, `site`).
    pub fn wait_and_redeem(&self, anon: &Client) -> Result<Value> {
        let code = wait_for_code(&self.listener, &self.state)?;
        redeem(anon, &code, &self.pkce.verifier)
    }
}

/// Step 1: open the port and register the request with the site.
pub fn start(anon: &Client, request: &LoginRequest) -> Result<Pending> {
    let listener = TcpListener::bind(("127.0.0.1", request.port.unwrap_or(0))).map_err(|e| {
        Error::Other(format!(
            "cannot open a local port for the sign-in callback: {e}"
        ))
    })?;
    let port = listener.local_addr()?.port();
    let state = request.state.clone().unwrap_or_else(random_state);
    let pkce = Pkce::generate();

    let body = json!({
        "name": request.name,
        "scopes": request.scopes,
        "port": port,
        "state": state,
        "code_challenge": pkce.challenge,
    });
    let response = anon.anonymous_post_preserving_redirects(START_PATH, &body)?;
    let request_id = response
        .body
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("login start response had no request_id".into()))?
        .to_string();
    let authorize_url = response
        .body
        .get("authorize_url")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Other("login start response had no authorize_url".into()))?
        .to_string();
    let expires_in = response
        .body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(600);

    Ok(Pending {
        listener,
        state,
        pkce,
        request_id,
        authorize_url,
        expires_in,
    })
}

/// Step 3: swap the one-time code for a key.
pub fn redeem(anon: &Client, code: &str, verifier: &str) -> Result<Value> {
    let body = json!({ "code": code, "code_verifier": verifier });
    Ok(anon
        .anonymous_post_preserving_redirects(TOKEN_PATH, &body)?
        .body)
}

/// Whole flow for the command: start, open the browser, wait, redeem.
pub fn login(anon: &Client, request: &LoginRequest, open_browser: bool) -> Result<Value> {
    let pending = start(anon, request)?;
    if open_browser {
        eprintln!(
            "Opening browser approval (valid for {} minutes). If it does not open, use this URL:\n\n  {}\n",
            pending.expires_in / 60,
            pending.authorize_url
        );
        if open::that(pending.authorize_url.as_str()).is_err() {
            eprintln!(
                "Could not open a browser automatically. Open the URL above in an already signed-in browser."
            );
        }
    } else {
        eprintln!(
            "Browser launch skipped. Open this URL in an already signed-in browser to approve the CLI (valid for {} minutes):\n\n  {}\n",
            pending.expires_in / 60,
            pending.authorize_url
        );
    }
    eprintln!(
        "Waiting for browser approval on local port {}…",
        pending.port()
    );
    pending.wait_and_redeem(anon)
}

/// Accept connections until one carries `/callback?code=…&state=…`.
fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (stream, _) = listener.accept()?;
        if let Some(code) = handle_connection(stream, expected_state)? {
            return Ok(code);
        }
    }
}

fn handle_connection(mut stream: TcpStream, expected_state: &str) -> Result<Option<String>> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Drain headers so the browser sees a clean close.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok() && line != "\r\n" && !line.is_empty() {
        line.clear();
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    if !path.starts_with("/callback") {
        respond(&mut stream, 404, "Not found")?;
        return Ok(None);
    }
    let parsed = Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|e| Error::Other(format!("bad callback URL: {e}")))?;
    let get = |name: &str| {
        parsed
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
    };

    match get("state") {
        // No state at all: a stray hit (prefetch, reload of the bare path). Keep waiting.
        None => {
            respond(&mut stream, 404, "Not found")?;
            return Ok(None);
        }
        Some(state) if state != expected_state => {
            respond(
                &mut stream,
                400,
                "This approval could not be matched to a command-line tool that is waiting. Please close this window and try again from the terminal.",
            )?;
            return Err(Error::Other(
                "callback state did not match; try again".into(),
            ));
        }
        Some(_) => {}
    }
    if let Some(error) = get("error") {
        respond(
            &mut stream,
            400,
            "The request was denied and nothing was granted. You can close this window.",
        )?;
        let message = if error == "access_denied" {
            "the request was denied in the browser".to_string()
        } else {
            format!("the site reported: {error}")
        };
        return Err(Error::Other(format!("login refused: {message}")));
    }
    match get("code") {
        Some(code) if !code.is_empty() => {
            respond(
                &mut stream,
                200,
                "You're all set. The command-line tool has been approved and can now manage content on your Real Geeks site. You can close this window and return to the terminal.",
            )?;
            Ok(Some(code))
        }
        _ => {
            respond(&mut stream, 400, "Something went wrong with this approval. Please close this window and try again from the terminal.")?;
            Err(Error::Other("callback had no code".into()))
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, text: &str) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Real Geeks</title></head>\
         <body style=\"font-family:system-ui;margin:3em\"><h2>Real Geeks</h2><p>{text}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636 appendix B
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_values_fit_server_rules() {
        let pkce = Pkce::generate();
        assert_eq!(pkce.verifier.len(), 43);
        assert_eq!(pkce.challenge.len(), 43);
        let state = random_state();
        assert!(
            state.len() >= 8
                && state
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
        );
    }
}
