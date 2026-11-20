//! Authentication manager and token storage.
//!
//! Tokens are stored, in order of preference:
//! 1. the OS secret service via `secret-tool` (GNOME Keyring / KWallet on Linux),
//! 2. a `0600` JSON file under the Persist data directory.
//!
//! The fallback is clearly reported so users know where their credentials live.

pub mod oauth;

pub use oauth::{OAuthClientConfig, TokenSet, UserInfo};

use crate::config::Paths;
use crate::error::{Error, Result};
use parking_lot::Mutex;
use std::path::PathBuf;

/// Pluggable token storage.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<TokenSet>>;
    fn save(&self, tokens: &TokenSet) -> Result<()>;
    fn clear(&self) -> Result<()>;
    /// Human readable description of where tokens are kept.
    fn describe(&self) -> String;
}

/// A `0600` JSON file.
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<TokenSet>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read(&self.path)?;
        match serde_json::from_slice(&raw) {
            Ok(t) => Ok(Some(t)),
            Err(_) => Ok(None),
        }
    }

    fn save(&self, tokens: &TokenSet) -> Result<()> {
        let data = serde_json::to_vec_pretty(tokens)?;
        crate::config::write_private(&self.path, &data)
    }

    fn clear(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("file ({})", self.path.display())
    }
}

/// Storage backed by the freedesktop secret service through `secret-tool`.
pub struct SecretToolStore {
    fallback: FileTokenStore,
}

const SECRET_SERVICE: &str = "persist";
const SECRET_ACCOUNT: &str = "google-oauth";

impl SecretToolStore {
    pub fn new(fallback_path: PathBuf) -> Self {
        Self {
            fallback: FileTokenStore::new(fallback_path),
        }
    }

    fn available() -> bool {
        std::process::Command::new("secret-tool")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

impl TokenStore for SecretToolStore {
    fn load(&self) -> Result<Option<TokenSet>> {
        if !Self::available() {
            return self.fallback.load();
        }
        let out = std::process::Command::new("secret-tool")
            .args(["lookup", "service", SECRET_SERVICE, "account", SECRET_ACCOUNT])
            .output();
        match out {
            Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                match serde_json::from_slice(&o.stdout) {
                    Ok(t) => Ok(Some(t)),
                    Err(_) => self.fallback.load(),
                }
            }
            // Nothing stored in the keyring yet: fall back to the file (which
            // may hold tokens from before a keyring became available).
            _ => self.fallback.load(),
        }
    }

    fn save(&self, tokens: &TokenSet) -> Result<()> {
        let data = serde_json::to_vec(tokens)?;
        if Self::available() {
            use std::io::Write;
            let mut child = std::process::Command::new("secret-tool")
                .args([
                    "store",
                    "--label=Persist Google OAuth",
                    "service",
                    SECRET_SERVICE,
                    "account",
                    SECRET_ACCOUNT,
                ])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&data)?;
            }
            let status = child.wait()?;
            if status.success() {
                // Also keep the file in sync as a fallback and remove any
                // stale copy is unnecessary; the keyring is authoritative.
                let _ = self.fallback.save(tokens);
                return Ok(());
            }
        }
        self.fallback.save(tokens)
    }

    fn clear(&self) -> Result<()> {
        if Self::available() {
            let _ = std::process::Command::new("secret-tool")
                .args(["clear", "service", SECRET_SERVICE, "account", SECRET_ACCOUNT])
                .output();
        }
        self.fallback.clear()
    }

    fn describe(&self) -> String {
        if Self::available() {
            "system keyring (secret-tool)".to_string()
        } else {
            self.fallback.describe()
        }
    }
}

/// An in-memory store, used by tests.
#[derive(Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<Option<TokenSet>>,
}

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<TokenSet>> {
        Ok(self.tokens.lock().clone())
    }
    fn save(&self, tokens: &TokenSet) -> Result<()> {
        *self.tokens.lock() = Some(tokens.clone());
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        *self.tokens.lock() = None;
        Ok(())
    }
    fn describe(&self) -> String {
        "memory".to_string()
    }
}

/// Coordinates OAuth client configuration, token storage and refreshing.
pub struct AuthManager {    cfg: OAuthClientConfig,
    store: Box<dyn TokenStore>,
    http: reqwest::Client,
    cache: Mutex<Option<TokenSet>>,
}

impl AuthManager {
    pub fn new(cfg: OAuthClientConfig, store: Box<dyn TokenStore>, http: reqwest::Client) -> Self {
        Self {
            cfg,
            store,
            http,
            cache: Mutex::new(None),
        }
    }

    /// Build an auth manager using the standard paths and the preferred token
    /// store for this platform.
    pub fn load(paths: &Paths) -> Result<Self> {
        let cfg = OAuthClientConfig::load(&paths.oauth_config_file())?;
        let store: Box<dyn TokenStore> =
            Box::new(SecretToolStore::new(paths.credentials_file()));
        Ok(Self::new(cfg, store, reqwest::Client::new()))
    }

    /// Build an auth manager with an explicit store (tests).
    pub fn with_store(cfg: OAuthClientConfig, store: Box<dyn TokenStore>) -> Self {
        Self::new(cfg, store, reqwest::Client::new())
    }

    pub fn client_config(&self) -> &OAuthClientConfig {
        &self.cfg
    }

    pub fn store_description(&self) -> String {
        self.store.describe()
    }

    /// Whether tokens are present (they may still need refreshing).
    pub fn is_connected(&self) -> bool {
        self.cache.lock().is_some() || matches!(self.store.load(), Ok(Some(_)))
    }

    /// Run the interactive OAuth flow and persist the resulting tokens.
    pub async fn connect(&self) -> Result<UserInfo> {
        let tokens = oauth::run_loopback_flow(&self.cfg, &self.http).await?;
        let info = oauth::fetch_userinfo(&self.http, &tokens.access_token).await?;
        self.store.save(&tokens)?;
        *self.cache.lock() = Some(tokens);
        Ok(info)
    }

    /// Store tokens obtained elsewhere (e.g. by a future device flow).
    pub fn store_tokens(&self, tokens: TokenSet) -> Result<()> {
        self.store.save(&tokens)?;
        *self.cache.lock() = Some(tokens);
        Ok(())
    }

    /// Forget the stored credentials.
    pub fn disconnect(&self) -> Result<()> {
        *self.cache.lock() = None;
        self.store.clear()
    }

    /// Return a valid access token, refreshing it if necessary.
    pub async fn access_token(&self) -> Result<String> {
        // Drop the cache guard before the `None` arm: parking_lot's Mutex is
        // not reentrant, and a temporary guard would otherwise live until the
        // end of the match, deadlocking on the second lock below.
        let cached = { self.cache.lock().clone() };
        let current = match cached {
            Some(t) => Some(t),
            None => {
                let t = self.store.load()?;
                if let Some(t) = &t {
                    *self.cache.lock() = Some(t.clone());
                }
                t
            }
        };
        let Some(tokens) = current else {
            return Err(Error::NotConnected);
        };
        if !tokens.is_expired(60) {
            return Ok(tokens.access_token);
        }
        // Refresh.
        let Some(refresh) = tokens.refresh_token.clone() else {
            return Err(Error::TokenExpired);
        };
        let refreshed = oauth::refresh_access_token(&self.cfg, &self.http, &refresh).await?;
        let token = refreshed.access_token.clone();
        self.store.save(&refreshed)?;
        *self.cache.lock() = Some(refreshed);
        Ok(token)
    }

    /// Fetch the connected account's profile, refreshing if needed.
    pub async fn account(&self) -> Result<UserInfo> {
        let token = self.access_token().await?;
        oauth::fetch_userinfo(&self.http, &token).await
    }
}

#[async_trait::async_trait]
impl crate::drive::TokenProvider for AuthManager {
    async fn access_token(&self) -> Result<String> {
        // Fully qualified to avoid recursing into this trait method.
        AuthManager::access_token(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileTokenStore::new(dir.path().join("creds.json"));
        assert!(store.load().unwrap().is_none());
        let t = TokenSet {
            access_token: "x".into(),
            refresh_token: Some("y".into()),
            expires_at: Some(crate::util::now_unix() + 3600),
            scope: None,
            token_type: "Bearer".into(),
        };
        store.save(&t).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), t);
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn credentials_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.json");
        let store = FileTokenStore::new(path.clone());
        store
            .save(&TokenSet {
                access_token: "x".into(),
                refresh_token: None,
                expires_at: None,
                scope: None,
                token_type: "Bearer".into(),
            })
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
