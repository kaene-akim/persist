//! The session facade.
//!
//! [`Session`] wires together configuration, the local database, authentication,
//! the storage backend and the repository, and exposes the high level operations
//! that both the CLI and the desktop application use. Keeping this logic here
//! (rather than in either interface) is what guarantees the two interfaces
//! behave identically.

use crate::auth::{AuthManager, UserInfo};
use crate::config::{AppConfig, BackendKind, Paths};
use crate::db::{ActivityKind, ActivityStatus, CachedSnapshot, Db};
use crate::engine::{self, BackupOptions, Progress};
use crate::error::{Error, Result};
use crate::model::{
    BackupPlan, BackupResult, ChangeSet, Quota, RepoMeta, RepoUsage, RootConfig, Settings, Snapshot,
};
use crate::repo::Repository;
use crate::storage::{local::LocalStorage, StorageRef};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A combined view of provider quota and Persist's own usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<Quota>,
    pub usage: RepoUsage,
}

/// The top level entry point for both interfaces.
pub struct Session {
    pub paths: Paths,
    pub config: AppConfig,
    pub db: Db,
    auth: Option<Arc<AuthManager>>,
    storage: Mutex<Option<StorageRef>>,
    repo: Mutex<Option<Arc<Repository>>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("backend", &self.config.backend)
            .field("active_repo", &self.config.active_repo)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Open a session using explicit paths.
    pub fn open(paths: Paths) -> Result<Self> {
        paths.ensure()?;
        let config = AppConfig::load(&paths)?;
        let db = Db::open(&paths.db_file())?;
        Ok(Self {
            paths,
            config,
            db,
            auth: None,
            storage: Mutex::new(None),
            repo: Mutex::new(None),
        })
    }

    /// Open a session using the default locations.
    pub fn open_default() -> Result<Self> {
        Self::open(Paths::resolve()?)
    }

    /// Build (or reuse) the authentication manager.
    pub fn auth(&mut self) -> Result<Arc<AuthManager>> {
        if let Some(a) = &self.auth {
            return Ok(a.clone());
        }
        let auth = Arc::new(AuthManager::load(&self.paths)?);
        self.auth = Some(auth.clone());
        Ok(auth)
    }

    /// Build an auth manager even when no OAuth client is configured yet.
    pub fn auth_opt(&self) -> Option<Arc<AuthManager>> {
        self.auth.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.auth.as_ref().map(|a| a.is_connected()).unwrap_or(false)
    }

    /// Run the interactive Google OAuth flow.
    pub async fn connect_google(&mut self) -> Result<UserInfo> {
        let auth = self.auth()?;
        let info = auth.connect().await?;
        self.db.add_activity(
            ActivityKind::Auth,
            ActivityStatus::Success,
            None,
            &format!("Connected Google account {}", info.email),
            serde_json::json!({ "email": info.email }),
        )?;
        Ok(info)
    }

    /// Forget the stored credentials.
    pub fn disconnect_google(&mut self) -> Result<()> {
        if let Some(auth) = &self.auth {
            auth.disconnect()?;
        }
        self.db.add_activity(
            ActivityKind::Auth,
            ActivityStatus::Success,
            None,
            "Disconnected Google account",
            serde_json::json!({}),
        )?;
        Ok(())
    }

    /// Fetch the connected account profile.
    pub async fn account(&self) -> Result<UserInfo> {
        let auth = self
            .auth
            .clone()
            .ok_or(Error::NotConnected)?;
        auth.account().await
    }

    // -- Settings and configuration ----------------------------------------

    pub fn settings(&self) -> Settings {
        self.db.get_settings().unwrap_or_default()
    }

    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        self.db.set_settings(settings)
    }

    pub fn save_config(&mut self) -> Result<()> {
        self.config.save(&self.paths)
    }

    pub fn roots(&self) -> Result<Vec<RootConfig>> {
        self.db.list_roots()
    }

    pub fn add_root(&self, mut root: RootConfig) -> Result<RootConfig> {
        if root.root_id.is_empty() {
            root.root_id = crate::util::new_id();
        }
        if root.remote_path.is_empty() {
            root.remote_path = slugify(&root.name);
        }
        if root.created_at.timestamp() == 0 {
            root.created_at = Utc::now();
        }
        self.db.upsert_root(&root)?;
        self.db.add_activity(
            ActivityKind::AddRoot,
            ActivityStatus::Success,
            Some(&root.root_id),
            &format!("Added directory {}", root.name),
            serde_json::json!({ "path": root.local_path }),
        )?;
        Ok(root)
    }

    pub fn remove_root(&self, root_id: &str) -> Result<()> {
        self.db.delete_root(root_id)?;
        self.db.add_activity(
            ActivityKind::RemoveRoot,
            ActivityStatus::Success,
            Some(root_id),
            "Removed directory from management (cloud history preserved)",
            serde_json::json!({}),
        )?;
        Ok(())
    }

    pub fn update_root(&self, root: &RootConfig) -> Result<()> {
        self.db.upsert_root(root)
    }

    // -- Storage ------------------------------------------------------------

    /// Build (or reuse) the storage backend.
    pub async fn storage(&self) -> Result<StorageRef> {
        if let Some(s) = self.storage.lock().await.clone() {
            return Ok(s);
        }
        let storage: StorageRef = match self.config.backend {
            BackendKind::GoogleDrive => {
                let auth = self
                    .auth
                    .clone()
                    .ok_or_else(|| Error::Config("Google Drive backend requires authentication".into()))?;
                if !auth.is_connected() {
                    return Err(Error::NotConnected);
                }
                let drive = crate::drive::DriveStorage::new(auth, self.config.drive_folder.clone())
                    .with_chunk_size(self.settings().upload_chunk_size as usize);
                if let Some(id) = &self.config.drive_root_id {
                    drive.set_root_id(id.clone());
                }
                Arc::new(drive)
            }
            BackendKind::Local => {
                let path = self
                    .config
                    .local_path
                    .clone()
                    .ok_or_else(|| Error::Config("local backend requires a path".into()))?;
                Arc::new(LocalStorage::new(path))
            }
        };
        *self.storage.lock().await = Some(storage.clone());
        Ok(storage)
    }

    /// Drop cached storage (after a configuration change).
    pub async fn reset_storage(&self) {
        *self.storage.lock().await = None;
        *self.repo.lock().await = None;
    }

    /// Open the configured repository, if it exists.
    pub async fn repository(&self) -> Result<Arc<Repository>> {
        if let Some(r) = self.repo.lock().await.clone() {
            return Ok(r);
        }
        let storage = self.storage().await?;
        let repo = Repository::open(storage).await?;
        let repo = Arc::new(repo);
        *self.repo.lock().await = Some(repo.clone());
        Ok(repo)
    }

    /// Whether a repository already exists at the configured location.
    pub async fn repository_exists(&self) -> Result<bool> {
        let storage = self.storage().await?;
        storage.exists(crate::repo::layout::REPO_META).await
    }

    /// Initialise a new repository.
    pub async fn init_repository(&mut self, name: &str) -> Result<RepoMeta> {
        let storage = self.storage().await?;
        let repo = Repository::init(storage, name, None).await?;
        let meta = repo.meta().clone();
        self.config.active_repo = Some(meta.repo_id.clone());
        self.config.repo_name = meta.name.clone();
        self.save_config()?;
        *self.repo.lock().await = Some(Arc::new(repo));
        self.db.add_activity(
            ActivityKind::Init,
            ActivityStatus::Success,
            None,
            &format!("Initialised repository {}", meta.repo_id),
            serde_json::json!({ "name": meta.name }),
        )?;
        Ok(meta)
    }

    /// Discover and adopt an existing repository (used on a new machine).
    pub async fn clone_repository(
        &mut self,
        expected_repo_id: Option<&str>,
        destination: Option<PathBuf>,
    ) -> Result<RepoMeta> {
        let storage = self.storage().await?;
        let repo = Repository::open(storage).await?;
        let meta = repo.meta().clone();
        if let Some(expected) = expected_repo_id {
            if expected != meta.repo_id {
                return Err(Error::InvalidRepository(format!(
                    "found repository {} but expected {expected}",
                    meta.repo_id
                )));
            }
        }
        // Rebuild the object index from the authoritative snapshots, then cache
        // the history locally.
        repo.rebuild_index().await?;
        self.config.active_repo = Some(meta.repo_id.clone());
        self.config.repo_name = meta.name.clone();
        self.save_config()?;
        let repo = Arc::new(repo);
        *self.repo.lock().await = Some(repo.clone());
        self.sync_snapshot_cache().await?;

        // Register roots from the head snapshot so the user can pull straight
        // away. Local paths point at the destination when one was given.
        if let Some(head) = repo.head().await? {
            let snap = repo.load_snapshot(&head).await?;
            for root in &snap.roots {
                let local_path = match &destination {
                    Some(dest) => dest.join(&root.name),
                    None => PathBuf::from(&root.local_path),
                };
                let cfg = RootConfig {
                    root_id: root.root_id.clone(),
                    name: root.name.clone(),
                    local_path,
                    remote_path: root.remote_path.clone(),
                    exclusions: Vec::new(),
                    enabled: true,
                    created_at: Utc::now(),
                };
                self.db.upsert_root(&cfg)?;
            }
        }
        self.db.add_activity(
            ActivityKind::Clone,
            ActivityStatus::Success,
            None,
            &format!("Adopted repository {}", meta.repo_id),
            serde_json::json!({ "name": meta.name }),
        )?;
        Ok(meta)
    }

    /// Reload all snapshot manifests into the local cache.
    pub async fn sync_snapshot_cache(&self) -> Result<u64> {
        let repo = self.repository().await?;
        let ids = repo.list_snapshot_ids().await?;
        self.db.clear_snapshot_cache()?;
        let mut n = 0;
        for id in &ids {
            if let Ok(snap) = repo.load_snapshot(id).await {
                self.db.cache_snapshot(&snap)?;
                n += 1;
            }
        }
        Ok(n)
    }

    // -- Backup -------------------------------------------------------------

    /// Analyse a backup without uploading anything.
    pub async fn plan_backup(&self, only_root: Option<&str>) -> Result<BackupPlan> {
        let repo = self.repository().await?;
        let roots = self.roots()?;
        if roots.iter().all(|r| !r.enabled) {
            return Err(Error::Config(
                "no directories are configured for backup; add one first".into(),
            ));
        }
        let settings = self.settings();
        engine::plan_backup(&repo, &roots, &settings, only_root).await
    }

    /// Run a full backup: plan, (optionally) execute, then update caches.
    pub async fn run_backup(
        &self,
        opts: &BackupOptions,
        progress: &dyn Progress,
    ) -> Result<BackupResult> {
        let repo = self.repository().await?;
        let roots = self.roots()?;
        let settings = self.settings();
        let plan = engine::plan_backup(&repo, &roots, &settings, opts.only_root.as_deref()).await?;
        if opts.dry_run {
            // Return a result describing what *would* happen, without committing.
            return Ok(BackupResult {
                snapshot_id: String::new(),
                parent: plan.head_snapshot.clone(),
                created_at: Utc::now(),
                files_backed_up: plan.total_files,
                logical_size: plan.total_logical_size,
                new_objects: plan.uploads.len() as u64,
                new_bytes: plan.upload_bytes,
                added: plan.change_set.added,
                modified: plan.change_set.modified,
                deleted: plan.change_set.deleted,
                unchanged: plan.change_set.unchanged,
                duration_secs: 0.0,
            });
        }
        let result = engine::execute_backup(&repo, &plan, &settings, progress).await?;
        // Cache the new snapshot.
        if let Ok(snap) = repo.load_snapshot(&result.snapshot_id).await {
            let _ = self.db.cache_snapshot(&snap);
        }
        self.db.add_activity(
            ActivityKind::Backup,
            ActivityStatus::Success,
            opts.only_root.as_deref(),
            &format!(
                "Backed up {} file(s), {} new object(s)",
                result.files_backed_up, result.new_objects
            ),
            serde_json::to_value(&result).unwrap_or(serde_json::json!({})),
        )?;
        Ok(result)
    }

    /// Compare the current local state with the head snapshot.
    pub async fn diff_local(&self, only_root: Option<&str>) -> Result<ChangeSet> {
        let plan = self.plan_backup(only_root).await?;
        Ok(plan.change_set)
    }

    /// Compare two snapshots.
    pub async fn diff_snapshots(&self, a: &str, b: &str) -> Result<ChangeSet> {
        let repo = self.repository().await?;
        let sa = repo.load_snapshot(a).await?;
        let sb = repo.load_snapshot(b).await?;
        Ok(engine::diff::diff_snapshots(Some(&sa), &sb))
    }

    // -- History ------------------------------------------------------------

    /// List cached snapshots, newest first. Syncs the cache first if empty.
    pub async fn snapshots(&self, limit: u32) -> Result<Vec<CachedSnapshot>> {
        if self.db.snapshot_cache_len()? == 0 {
            let _ = self.sync_snapshot_cache().await;
        }
        self.db.list_cached_snapshots(limit)
    }

    pub async fn head_snapshot(&self) -> Result<Option<Snapshot>> {
        let repo = self.repository().await?;
        match repo.head().await? {
            Some(id) => Ok(Some(repo.load_snapshot(&id).await?)),
            None => Ok(None),
        }
    }

    // -- Restore ------------------------------------------------------------

    pub async fn plan_restore(
        &self,
        opts: &engine::RestoreOptions,
    ) -> Result<(Snapshot, engine::RestorePlan)> {
        let repo = self.repository().await?;
        engine::plan_restore(&repo, opts).await
    }

    pub async fn restore(
        &self,
        opts: &engine::RestoreOptions,
        progress: &dyn Progress,
    ) -> Result<engine::restore::RestoreReport> {
        let repo = self.repository().await?;
        let (_snap, plan) = engine::plan_restore(&repo, opts).await?;
        let report = engine::restore(&repo, &plan, opts, progress).await?;
        if !opts.dry_run {
            self.db.add_activity(
                ActivityKind::Restore,
                ActivityStatus::Success,
                opts.root_id.as_deref(),
                &format!(
                    "Restored {} file(s) from snapshot {}",
                    report.files_restored, report.snapshot_id
                ),
                serde_json::to_value(&report).unwrap_or(serde_json::json!({})),
            )?;
        }
        Ok(report)
    }

    // -- Verification and storage ------------------------------------------

    pub async fn verify(&self, deep: bool, progress: &dyn Progress) -> Result<engine::VerifyReport> {
        let repo = self.repository().await?;
        let report = engine::verify_repository(&repo, deep, progress).await?;
        let status = if report.ok {
            ActivityStatus::Success
        } else {
            ActivityStatus::Failed
        };
        self.db.add_activity(
            ActivityKind::Verify,
            status,
            None,
            &format!(
                "Verified {} snapshot(s), {} object(s)",
                report.snapshots_checked, report.objects_checked
            ),
            serde_json::to_value(&report).unwrap_or(serde_json::json!({})),
        )?;
        Ok(report)
    }

    /// Provider quota and Persist's own usage.
    pub async fn storage_info(&self) -> Result<StorageInfo> {
        let storage = self.storage().await?;
        let quota = storage.quota().await.unwrap_or(None);
        let usage = match self.repository().await {
            Ok(repo) => repo.usage().await.unwrap_or_default(),
            Err(_) => RepoUsage::default(),
        };
        Ok(StorageInfo {
            backend: storage.backend_name().to_string(),
            quota,
            usage,
        })
    }

    /// Recent activity.
    pub fn activity(&self, limit: u32) -> Result<Vec<crate::db::ActivityEntry>> {
        self.db.list_activity(limit)
    }
}

/// Turn a name into a safe repository-relative path component.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "directory".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Documents"), "my-documents");
        assert_eq!(slugify("  ..weird//name  "), "weird-name");
        assert_eq!(slugify("***"), "directory");
    }
}
