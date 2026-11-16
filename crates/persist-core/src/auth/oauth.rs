//! OAuth 2.0 support for Google.
//!
//! Persist uses the *installed application* flow with a loopback redirect and
//! PKCE. No client secret is ever embedded in the binary: the user supplies
//! their own OAuth client (see `docs/google-oauth.md`), either through the
//! environment or through `~/.config/persist/oauth.json`.

use crate::error::{Error, Result};
use crate::util;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const GOOGLE_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_USERINFO_URI: &str = "https://www.googleapis.com/oauth2/v3/userinfo";

/// Minimal set of scopes Persist needs:
/// * `drive.file` — access only to files Persist itself creates.
/// * `userinfo.email` / `openid` — to display which account is connected.
pub fn default_scopes() -> Vec<String> {
    vec![
        "openid".to_string(),
        "https://www.googleapis.com/auth/userinfo.email".to_string(),
        "https://www.googleapis.com/auth/drive.file".to_string(),
    ]
}

/// OAuth client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientConfig {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    #[serde(default = "default_auth_uri")]
    pub auth_uri: String,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}

fn default_auth_uri() -> String {
    GOOGLE_AUTH_URI.to_string()
}
fn default_token_uri() -> String {
    GOOGLE_TOKEN_URI.to_string()
}

impl OAuthClientConfig {
    /// Load the OAuth client configuration from, in order of precedence:
    /// 1. the `PERSIST_CLIENT_ID` / `PERSIST_CLIENT_SECRET` environment variables,
    /// 2. the given JSON file.
    pub fn load(file: &Path) -> Result<Self> {
        if let Ok(client_id) = std::env::var("PERSIST_CLIENT_ID") {
            if !client_id.trim().is_empty() {
                let client_secret = std::env::var("PERSIST_CLIENT_SECRET").ok();
                return Ok(Self {
                    client_id,
                    client_secret,
                    scopes: default_scopes(),
                    auth_uri: GOOGLE_AUTH_URI.to_string(),
                    token_uri: GOOGLE_TOKEN_URI.to_string(),
                });
            }
        }
        if !file.exists() {
            return Err(Error::Config(format!(
                "no Google OAuth client configured. Set PERSIST_CLIENT_ID (and \
                 PERSIST_CLIENT_SECRET) or create {} — see docs/google-oauth.md",
                file.display()
            )));
        }
        let raw = std::fs::read(file)?;
        serde_json::from_slice(&raw)
            .map_err(|e| Error::Config(format!("{} is malformed: {e}", file.display())))
    }

    /// True when a client secret is configured. Google's desktop flow can work
    /// without one, but many client types require it.
    pub fn has_secret(&self) -> bool {
        self.client_secret
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }
}

/// The account details returned by Google's userinfo endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub picture: Option<String>,
}

/// Run the loopback authorization-code flow and return the resulting tokens.
pub async fn run_loopback_flow(cfg: &OAuthClientConfig, http: &reqwest::Client) -> Result<TokenSet> {
    // 1. Bind a loopback listener on an ephemeral port.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    // 2. Build PKCE parameters.
    let verifier = random_url_safe(48);
    let challenge = {
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
    };
    let state = random_url_safe(16);

    let scope = cfg.scopes.join(" ");
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}\
         &code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        cfg.auth_uri,
        urlencode(&cfg.client_id),
        urlencode(&redirect_uri),
        urlencode(&scope),
        urlencode(&challenge),
        urlencode(&state),
    );

    // 3. Present the URL and try to open a browser.
    println!("Open the following URL in your browser to authorise Persist:\n\n{auth_url}\n");
    let _ = open_browser(&auth_url);

    // 4. Wait for the redirect.
    let code = wait_for_code(listener, &state).await?;

    // 5. Exchange the code for tokens.
    exchange_code(cfg, http, &code, &redirect_uri, &verifier).await
}

async fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    // Accept a small number of connections so favicon requests etc. do not
    // derail the flow.
    for _ in 0..10 {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await?;
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let first_line = req.lines().next().unwrap_or_default();
        // GET /?code=...&state=... HTTP/1.1
        let query = first_line
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.split_once('?').map(|(_, q)| q.to_string()))
            .unwrap_or_default();
        let params = parse_query(&query);

        if let Some(err) = params.get("error") {
            let body = html_page("Authorisation failed", err);
            let _ = respond(&mut socket, 400, &body).await;
            return Err(Error::AuthFailed(err.clone()));
        }
        let (Some(code), Some(state)) = (params.get("code"), params.get("state")) else {
            let _ = respond(&mut socket, 400, "Missing parameters").await;
            continue;
        };
        if state != expected_state {
            let _ = respond(&mut socket, 400, "State mismatch").await;
            return Err(Error::AuthFailed("OAuth state mismatch (possible CSRF)".into()));
        }
        let body = html_page(
            "Persist is connected",
            "You can close this tab and return to Persist.",
        );
        let _ = respond(&mut socket, 200, &body).await;
        return Ok(code.clone());
    }
    Err(Error::AuthFailed("no authorisation code received".into()))
}

async fn exchange_code(
    cfg: &OAuthClientConfig,
    http: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenSet> {
    let mut form = vec![
        ("code", code.to_string()),
        ("client_id", cfg.client_id.clone()),
        ("redirect_uri", redirect_uri.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code_verifier", verifier.to_string()),
    ];
    if let Some(secret) = &cfg.client_secret {
        if !secret.is_empty() {
            form.push(("client_secret", secret.clone()));
        }
    }
    let resp = http
        .post(&cfg.token_uri)
        .form(&form)
        .send()
        .await?;
    parse_token_response(resp).await
}

/// Refresh an access token using a refresh token.
pub async fn refresh_access_token(
    cfg: &OAuthClientConfig,
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenSet> {
    let mut form = vec![
        ("client_id", cfg.client_id.clone()),
        ("refresh_token", refresh_token.to_string()),
        ("grant_type", "refresh_token".to_string()),
    ];
    if let Some(secret) = &cfg.client_secret {
        if !secret.is_empty() {
            form.push(("client_secret", secret.clone()));
        }
    }
    let resp = http.post(&cfg.token_uri).form(&form).send().await?;
    let mut tokens = parse_token_response(resp).await?;
    // Google only returns a refresh token on the first consent; preserve the old one.
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.to_string());
    }
    Ok(tokens)
}

async fn parse_token_response(resp: reqwest::Response) -> Result<TokenSet> {
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(Error::AuthFailed(format!(
            "token endpoint returned {status}: {text}"
        )));
    }
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| Error::AuthFailed(format!("malformed token response: {e}")))?;
    let access_token = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::AuthFailed("token response had no access_token".into()))?
        .to_string();
    let expires_in = v.get("expires_in").and_then(|x| x.as_i64());
    Ok(TokenSet {
        access_token,
        refresh_token: v
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        expires_at: expires_in.map(|secs| util::now_unix() + secs),
        scope: v.get("scope").and_then(|x| x.as_str()).map(|s| s.to_string()),
        token_type: v
            .get("token_type")
            .and_then(|x| x.as_str())
            .unwrap_or("Bearer")
            .to_string(),
    })
}

/// Fetch the connected account's profile.
pub async fn fetch_userinfo(http: &reqwest::Client, access_token: &str) -> Result<UserInfo> {
    let resp = http
        .get(GOOGLE_USERINFO_URI)
        .bearer_auth(access_token)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(Error::AuthFailed(format!(
            "userinfo endpoint returned {status}: {text}"
        )));
    }
    serde_json::from_str(&text).map_err(|e| Error::AuthFailed(format!("malformed userinfo: {e}")))
}

/// Try to open a URL in the user's browser. Failures are non-fatal.
pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd").args(["/C", "start", url]).spawn()?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no browser opener available",
        ))
    }
}

// -- Token set -------------------------------------------------------------

/// A set of OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix timestamp at which the access token expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default = "default_bearer")]
    pub token_type: String,
}

fn default_bearer() -> String {
    "Bearer".to_string()
}

impl TokenSet {
    /// True when the access token is expired (or within `skew` seconds of it).
    pub fn is_expired(&self, skew: i64) -> bool {
        match self.expires_at {
            Some(exp) => util::now_unix() + skew >= exp,
            None => false,
        }
    }
}

// -- helpers ---------------------------------------------------------------

fn random_url_safe(n: usize) -> String {
    use rand::RngCore;
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| {
            (
                k.to_string(),
                percent_encoding::percent_decode_str(v)
                    .decode_utf8_lossy()
                    .to_string(),
            )
        })
        .collect()
}

fn html_page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:sans-serif;background:#1b2a20;color:#e8e0c9;\
         display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}\
         .card{{background:#243528;padding:2rem 3rem;border-radius:12px;\
         box-shadow:0 8px 30px rgba(0,0,0,.4);text-align:center}}h1{{margin:0 0 .5rem}}</style>\
         </head><body><div class=\"card\"><h1>{title}</h1><p>{message}</p></div></body></html>"
    )
}

async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_expiry() {
        let t = TokenSet {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: Some(util::now_unix() + 100),
            scope: None,
            token_type: "Bearer".into(),
        };
        assert!(!t.is_expired(60));
        assert!(t.is_expired(200));
    }

    #[test]
    fn query_parsing() {
        let p = parse_query("code=abc&state=xyz");
        assert_eq!(p.get("code").unwrap(), "abc");
        assert_eq!(p.get("state").unwrap(), "xyz");
    }
}
