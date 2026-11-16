//! Local configuration and filesystem locations.
//!
//! Persist keeps *no* essential data locally: everything needed to recover a
//! backup lives in the cloud repository. The local configuration and SQLite
//! database are caches and conveniences that can be rebuilt.

use crate::error::{Error, Result};
use crate::model::Settings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Resolved application directories.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    /// Resolve directories, honouring `PERSIST_HOME` (used by tests and by
    /// users who want a portable install).
    pub fn resolve() -> Result<Self> {
        if let Ok(home) = std::env::var("PERSIST_HOME") {
            let base = PathBuf::from(home);
            return Ok(Self {
                config_dir: base.join("config"),
                data_dir: base.join("data"),
                cache_dir: base.join("cache"),
            });
        }
        let config_dir = dirs::config_dir()
            .ok_or_else(|| Error::Config("could not determine config directory".into()))?
            .join("persist");
        let data_dir = dirs::data_dir()
            .ok_or_else(|| Error::Config("could not determine data directory".into()))?
            .join("persist");
        let cache_dir = dirs::cache_dir()
            .map(|d| d.join("persist"))
            .unwrap_or_else(|| data_dir.join("cache"));
        Ok(Self {
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("persist.db")
    }

    /// Fallback location for OAuth tokens when no system keyring is available.
    pub fn credentials_file(&self) -> PathBuf {
        self.data_dir.join("credentials.json")
    }

    /// Location of the Google OAuth client configuration.
    pub fn oauth_config_file(&self) -> PathBuf {
        self.config_dir.join("oauth.json")
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }
}

/// Which storage backend the local install is configured to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    GoogleDrive,
    Local,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::GoogleDrive => "google-drive",
            BackendKind::Local => "local",
        }
    }
}

/// The persisted local configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub backend: BackendKind,
    /// Repository id of the currently active repository, if any.
    #[serde(default)]
    pub active_repo: Option<String>,
    /// Human readable repository name.
    #[serde(default = "default_repo_name")]
    pub repo_name: String,
    /// Folder name created in Google Drive.
    #[serde(default = "default_drive_folder")]
    pub drive_folder: String,
    /// Cached Drive folder id for the app root, to avoid repeated lookups.
    #[serde(default)]
    pub drive_root_id: Option<String>,
    /// Path used when `backend == Local`.
    #[serde(default)]
    pub local_path: Option<PathBuf>,
    #[serde(default)]
    pub settings: Settings,
}

fn default_repo_name() -> String {
    "Persist".to_string()
}

fn default_drive_folder() -> String {
    "Persist".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            backend: BackendKind::GoogleDrive,
            active_repo: None,
            repo_name: default_repo_name(),
            drive_folder: default_drive_folder(),
            drive_root_id: None,
            local_path: None,
            settings: Settings::default(),
        }
    }
}

impl AppConfig {
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = paths.config_file();
        if !file.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read(&file)?;
        serde_json::from_slice(&raw)
            .map_err(|e| Error::Config(format!("{} is malformed: {e}", file.display())))
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let data = serde_json::to_vec_pretty(self)?;
        let file = paths.config_file();
        write_private(&file, &data)?;
        Ok(())
    }
}

/// Write a file with owner-only permissions where the platform supports it.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_config() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("c"),
            data_dir: dir.path().join("d"),
            cache_dir: dir.path().join("k"),
        };
        let mut cfg = AppConfig::default();
        cfg.repo_name = "test".into();
        cfg.save(&paths).unwrap();
        let loaded = AppConfig::load(&paths).unwrap();
        assert_eq!(loaded.repo_name, "test");
        assert_eq!(loaded.backend, BackendKind::GoogleDrive);
    }
}
