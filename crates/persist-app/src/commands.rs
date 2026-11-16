//! Tauri commands: the entire surface the desktop frontend talks to.
//!
//! Every command delegates to the shared [`Session`](persist_core::session::Session)
//! so the desktop app and the CLI behave identically. Long running commands
//! report progress through [`EventProgress`] and can be cancelled.

use crate::progress::EventProgress;
use crate::state::AppState;
use persist_core::config::BackendKind;
use persist_core::db::{ActivityEntry, CachedSnapshot};
use persist_core::engine::{
    BackupOptions, OverwritePolicy, RestoreOptions, RestorePlan, VerifyReport,
};
use persist_core::error::Error;
use persist_core::model::{
    BackupPlan, BackupResult, ChangeSet, RepoMeta, RootConfig, Settings, Snapshot,
};
use persist_core::session::StorageInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

/// Progress event name shared with the frontend.
const PROGRESS_EVENT: &str = "persist://progress";

/// A structured error returned to the frontend.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        Self {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

type CmdResult<T> = std::result::Result<T, CommandError>;

fn err(code: &str, message: impl Into<String>) -> CommandError {
    CommandError {
        code: code.to_string(),
        message: message.into(),
    }
}

/// High level application status, shown on the dashboard.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub backend: String,
    pub repo_name: String,
    pub active_repo: Option<String>,
    pub connected: bool,
    pub initialised: bool,
    pub local_path: Option<String>,
    pub config_dir: String,
    pub data_dir: String,
    pub drive_folder: String,
}

// -- application / configuration -------------------------------------------

#[tauri::command]
pub async fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let session = state.session.lock().await;
    let initialised = session.repository_exists().await.unwrap_or(false);
    Ok(AppInfo {
        version: persist_core::VERSION.to_string(),
        backend: session.config.backend.as_str().to_string(),
        repo_name: session.config.repo_name.clone(),
        active_repo: session.config.active_repo.clone(),
        connected: session.is_connected(),
        initialised,
        local_path: session
            .config
            .local_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        config_dir: session.paths.config_dir.to_string_lossy().to_string(),
        data_dir: session.paths.data_dir.to_string_lossy().to_string(),
        drive_folder: session.config.drive_folder.clone(),
    })
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let session = state.session.lock().await;
    Ok(serde_json::to_value(&session.config).unwrap_or(serde_json::json!({})))
}

#[tauri::command]
pub async fn set_backend(
    state: State<'_, AppState>,
    backend: String,
    local_path: Option<String>,
) -> CmdResult<()> {
    let mut session = state.session.lock().await;
    session.config.backend = match backend.as_str() {
        "local" => BackendKind::Local,
        "google_drive" | "google-drive" | "drive" => BackendKind::GoogleDrive,
        other => return Err(err("config", format!("unknown backend `{other}`"))),
    };
    if let Some(path) = local_path {
        session.config.local_path = Some(PathBuf::from(path));
    }
    session.save_config()?;
    session.reset_storage().await;
    Ok(())
}

#[tauri::command]
pub async fn set_drive_folder(state: State<'_, AppState>, folder: String) -> CmdResult<()> {
    let mut session = state.session.lock().await;
    session.config.drive_folder = folder;
    session.config.drive_root_id = None;
    session.save_config()?;
    session.reset_storage().await;
    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    let session = state.session.lock().await;
    Ok(session.settings())
}

#[tauri::command]
pub async fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    let session = state.session.lock().await;
    session.set_settings(&settings)?;
    Ok(())
}

// -- authentication ---------------------------------------------------------

#[tauri::command]
pub async fn account(state: State<'_, AppState>) -> CmdResult<Option<persist_core::auth::UserInfo>> {
    let session = state.session.lock().await;
    if !session.is_connected() {
        return Ok(None);
    }
    match session.account().await {
        Ok(info) => Ok(Some(info)),
        Err(Error::NotConnected) | Err(Error::AuthRequired(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
pub async fn connect_google(
    state: State<'_, AppState>,
) -> CmdResult<persist_core::auth::UserInfo> {
    let mut session = state.session.lock().await;
    Ok(session.connect_google().await?)
}

#[tauri::command]
pub async fn disconnect_google(state: State<'_, AppState>) -> CmdResult<()> {
    let mut session = state.session.lock().await;
    session.disconnect_google()?;
    session.reset_storage().await;
    Ok(())
}

// -- repository lifecycle ---------------------------------------------------

#[tauri::command]
pub async fn init_repository(
    state: State<'_, AppState>,
    name: String,
    local_path: Option<String>,
) -> CmdResult<RepoMeta> {
    let mut session = state.session.lock().await;
    if let Some(path) = local_path {
        session.config.backend = BackendKind::Local;
        session.config.local_path = Some(PathBuf::from(path));
        session.save_config()?;
        session.reset_storage().await;
    }
    if session.repository_exists().await.unwrap_or(false) {
        return Err(Error::Config(
            "a Persist repository already exists at this location".into(),
        )
        .into());
    }
    Ok(session.init_repository(&name).await?)
}

#[tauri::command]
pub async fn clone_repository(
    state: State<'_, AppState>,
    repository_id: Option<String>,
    destination: Option<String>,
) -> CmdResult<RepoMeta> {
    let mut session = state.session.lock().await;
    let dest = destination.map(PathBuf::from);
    Ok(session
        .clone_repository(repository_id.as_deref(), dest)
        .await?)
}

// -- managed directories ----------------------------------------------------

#[tauri::command]
pub async fn list_roots(state: State<'_, AppState>) -> CmdResult<Vec<RootConfig>> {
    let session = state.session.lock().await;
    Ok(session.roots()?)
}

#[tauri::command]
pub async fn add_root(
    state: State<'_, AppState>,
    path: String,
    remote: Option<String>,
    name: Option<String>,
    exclude: Vec<String>,
) -> CmdResult<RootConfig> {
    let session = state.session.lock().await;
    let raw = PathBuf::from(&path);
    let canonical =
        std::fs::canonicalize(&raw).map_err(|_| Error::DirectoryMissing(raw.clone()))?;
    if !canonical.is_dir() {
        return Err(err(
            "config",
            format!("{} is not a directory", canonical.display()),
        ));
    }
    let dir_name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "directory".to_string());
    let name = name.unwrap_or(dir_name);
    let remote_path = remote.unwrap_or_else(|| persist_core::session::slugify(&name));
    let root = RootConfig {
        root_id: String::new(),
        name,
        local_path: canonical,
        remote_path,
        exclusions: exclude,
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    Ok(session.add_root(root)?)
}

#[tauri::command]
pub async fn remove_root(state: State<'_, AppState>, spec: String) -> CmdResult<String> {
    let session = state.session.lock().await;
    let root = resolve_root(&session, &spec)?;
    session.remove_root(&root.root_id)?;
    Ok(root.root_id)
}

#[tauri::command]
pub async fn update_root(state: State<'_, AppState>, root: RootConfig) -> CmdResult<()> {
    let session = state.session.lock().await;
    session.update_root(&root)?;
    Ok(())
}

// -- backup -----------------------------------------------------------------

#[tauri::command]
pub async fn plan_backup(
    state: State<'_, AppState>,
    only_root: Option<String>,
) -> CmdResult<BackupPlan> {
    let session = state.session.lock().await;
    Ok(session.plan_backup(only_root.as_deref()).await?)
}

#[tauri::command]
pub async fn run_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    only_root: Option<String>,
    dry_run: bool,
    message: Option<String>,
) -> CmdResult<BackupResult> {
    let session = state.session.lock().await;
    state.cancel.store(false, Ordering::SeqCst);
    let progress = EventProgress::new(app, PROGRESS_EVENT, state.cancel.clone());
    let opts = BackupOptions {
        dry_run,
        only_root,
        message,
    };
    Ok(session.run_backup(&opts, &progress).await?)
}

// -- history ----------------------------------------------------------------

#[tauri::command]
pub async fn list_snapshots(
    state: State<'_, AppState>,
    limit: u32,
) -> CmdResult<Vec<CachedSnapshot>> {
    let session = state.session.lock().await;
    Ok(session.snapshots(limit).await?)
}

#[tauri::command]
pub async fn get_snapshot(
    state: State<'_, AppState>,
    snapshot_id: Option<String>,
) -> CmdResult<Option<Snapshot>> {
    let session = state.session.lock().await;
    let repo = session.repository().await?;
    let id = match snapshot_id {
        Some(id) => id,
        None => match repo.head().await? {
            Some(id) => id,
            None => return Ok(None),
        },
    };
    match repo.load_snapshot(&id).await {
        Ok(snap) => Ok(Some(snap)),
        Err(Error::SnapshotNotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
pub async fn diff_local(
    state: State<'_, AppState>,
    only_root: Option<String>,
) -> CmdResult<ChangeSet> {
    let session = state.session.lock().await;
    Ok(session.diff_local(only_root.as_deref()).await?)
}

#[tauri::command]
pub async fn diff_snapshots(
    state: State<'_, AppState>,
    snapshot_a: String,
    snapshot_b: String,
) -> CmdResult<ChangeSet> {
    let session = state.session.lock().await;
    Ok(session.diff_snapshots(&snapshot_a, &snapshot_b).await?)
}

// -- restore ----------------------------------------------------------------

/// The restore request as sent by the frontend.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub root_id: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub destination: Option<String>,
    /// One of `refuse` (default), `overwrite`, `skip`, `rename`.
    #[serde(default)]
    pub overwrite: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

impl RestoreRequest {
    fn into_options(self) -> CmdResult<RestoreOptions> {
        let overwrite = match self.overwrite.as_deref() {
            None | Some("refuse") => OverwritePolicy::Refuse,
            Some("overwrite") => OverwritePolicy::Overwrite,
            Some("skip") => OverwritePolicy::Skip,
            Some("rename") => OverwritePolicy::Rename,
            Some(other) => {
                return Err(err(
                    "config",
                    format!("unknown overwrite policy `{other}`"),
                ))
            }
        };
        Ok(RestoreOptions {
            snapshot_id: self.snapshot_id,
            root_id: self.root_id,
            paths: self.paths,
            destination: self.destination.map(PathBuf::from),
            overwrite,
            dry_run: self.dry_run,
            preserve_metadata: true,
        })
    }
}

#[tauri::command]
pub async fn plan_restore(
    state: State<'_, AppState>,
    request: RestoreRequest,
) -> CmdResult<RestorePlan> {
    let session = state.session.lock().await;
    let opts = request.into_options()?;
    let (_snapshot, plan) = session.plan_restore(&opts).await?;
    Ok(plan)
}

#[tauri::command]
pub async fn run_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RestoreRequest,
) -> CmdResult<persist_core::engine::restore::RestoreReport> {
    let session = state.session.lock().await;
    state.cancel.store(false, Ordering::SeqCst);
    let opts = request.into_options()?;
    let progress = EventProgress::new(app, PROGRESS_EVENT, state.cancel.clone());
    Ok(session.restore(&opts, &progress).await?)
}

// -- verification, storage, activity ---------------------------------------

#[tauri::command]
pub async fn verify(app: AppHandle, state: State<'_, AppState>, deep: bool) -> CmdResult<VerifyReport> {
    let session = state.session.lock().await;
    state.cancel.store(false, Ordering::SeqCst);
    let progress = EventProgress::new(app, PROGRESS_EVENT, state.cancel.clone());
    Ok(session.verify(deep, &progress).await?)
}

#[tauri::command]
pub async fn storage_info(state: State<'_, AppState>) -> CmdResult<StorageInfo> {
    let session = state.session.lock().await;
    Ok(session.storage_info().await?)
}

#[tauri::command]
pub async fn list_activity(
    state: State<'_, AppState>,
    limit: u32,
) -> CmdResult<Vec<ActivityEntry>> {
    let session = state.session.lock().await;
    Ok(session.activity(limit)?)
}

#[tauri::command]
pub async fn cancel_operation(state: State<'_, AppState>) -> CmdResult<()> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

// -- helpers ----------------------------------------------------------------

/// Resolve a directory spec to a managed root by id, name or path.
fn resolve_root(session: &persist_core::session::Session, spec: &str) -> CmdResult<RootConfig> {
    let roots = session.roots()?;
    if let Some(r) = roots
        .iter()
        .find(|r| r.root_id == spec || r.name == spec || r.local_path.to_string_lossy() == spec)
    {
        return Ok(r.clone());
    }
    if let Ok(canon) = std::fs::canonicalize(spec) {
        if let Some(r) = roots.iter().find(|r| r.local_path == canon) {
            return Ok(r.clone());
        }
    }
    Err(Error::RootNotFound(spec.to_string()).into())
}
