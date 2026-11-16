//! Google Drive storage backend.
//!
//! [`DriveStorage`] adapts the flat, id-based Drive namespace to the
//! hierarchical [`Storage`] contract used by the backup engine. Logical
//! directory paths are resolved to Drive folder ids lazily and cached in memory;
//! every lookup falls back to a name query so a fresh install can rediscover an
//! existing repository purely from the cloud.

pub mod client;

pub use client::{DriveClient, DriveFile, DriveQuota, TokenProvider};

use crate::error::{Error, Result};
use crate::model::Quota;
use crate::storage::{DirEntry, Storage};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The `appProperties` marker attached to everything Persist creates.
fn marker(kind: &str) -> serde_json::Value {
    serde_json::json!({ "persist": kind })
}

/// A [`Storage`] implementation backed by Google Drive.
pub struct DriveStorage {
    client: DriveClient,
    root_name: String,
    root_id: Mutex<Option<String>>,
    folders: Mutex<HashMap<String, String>>,
    /// Serialises folder discovery/creation so concurrent uploads cannot create
    /// duplicate folders for the same path.
    folder_lock: tokio::sync::Mutex<()>,
}

impl DriveStorage {
    /// Create a backend that manages a top-level folder called `root_name`.
    pub fn new(tokens: Arc<dyn TokenProvider>, root_name: impl Into<String>) -> Self {
        Self {
            client: DriveClient::new(tokens),
            root_name: root_name.into(),
            root_id: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
            folder_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn with_chunk_size(mut self, bytes: usize) -> Self {
        self.client = self.client.clone().with_chunk_size(bytes);
        self
    }

    pub fn client(&self) -> &DriveClient {
        &self.client
    }

    /// Seed the root folder id (e.g. from local config) to avoid a lookup.
    pub fn set_root_id(&self, id: impl Into<String>) {
        *self.root_id.lock() = Some(id.into());
    }

    /// The resolved root folder id, creating the folder if necessary.
    pub async fn root_id(&self) -> Result<String> {
        if let Some(id) = self.root_id.lock().clone() {
            return Ok(id);
        }
        let _guard = self.folder_lock.lock().await;
        // Re-check after acquiring the lock.
        if let Some(id) = self.root_id.lock().clone() {
            return Ok(id);
        }
        // Look for the folder at the Drive root, then create it.
        let existing = self.client.find_child("root", &self.root_name).await?;
        let id = match existing {
            Some(f) if f.is_folder() => f.id,
            Some(_) => {
                return Err(Error::InvalidRepository(format!(
                    "a file named {:?} exists at the root of your Drive; \
                     please rename or remove it so Persist can create its folder",
                    self.root_name
                )))
            }
            None => self.client.create_folder("root", &self.root_name).await?.id,
        };
        *self.root_id.lock() = Some(id.clone());
        Ok(id)
    }

    /// Resolve the Drive folder id for a logical directory path, creating
    /// intermediate folders as needed.
    pub async fn ensure_folder(&self, path: &str) -> Result<String> {
        let path = path.trim_matches('/').to_string();
        if path.is_empty() {
            return self.root_id().await;
        }
        if let Some(id) = self.folders.lock().get(&path).cloned() {
            return Ok(id);
        }
        let _guard = self.folder_lock.lock().await;
        if let Some(id) = self.folders.lock().get(&path).cloned() {
            return Ok(id);
        }
        let (parent_path, name) = match path.rsplit_once('/') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), path.clone()),
        };
        let parent_id = Box::pin(self.ensure_folder(&parent_path)).await?;
        let folder = self.client.find_or_create_folder(&parent_id, &name).await?;
        self.folders.lock().insert(path, folder.id.clone());
        Ok(folder.id)
    }

    /// Resolve a file by logical path.
    pub async fn resolve_file(&self, path: &str) -> Result<Option<DriveFile>> {
        let (dir, name) = crate::storage::split_path(path);
        let parent = self.ensure_folder(&dir).await?;
        self.client.find_child(&parent, &name).await
    }
}

#[async_trait]
impl Storage for DriveStorage {
    fn backend_name(&self) -> &'static str {
        "google-drive"
    }

    async fn put(&self, path: &str, data: &[u8]) -> Result<()> {
        let (dir, name) = crate::storage::split_path(path);
        let parent = self.ensure_folder(&dir).await?;
        let kind = path_kind(path);
        match self.client.find_child(&parent, &name).await? {
            Some(existing) => {
                self.client
                    .update_content(&existing.id, "application/octet-stream", data)
                    .await?;
            }
            None => {
                self.client
                    .upload_new(&parent, &name, "application/octet-stream", data, marker(kind))
                    .await?;
            }
        }
        Ok(())
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        let file = self
            .resolve_file(path)
            .await?
            .ok_or_else(|| Error::MissingObject(path.to_string()))?;
        self.client.download(&file.id).await
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.resolve_file(path).await?.is_some())
    }

    async fn list(&self, dir: &str) -> Result<Vec<DirEntry>> {
        let folder = self.ensure_folder(dir).await?;
        let children = self.client.list_children(&folder).await?;
        let mut out: Vec<DirEntry> = children
            .into_iter()
            .map(|f| {
                let is_dir = f.is_folder();
                DirEntry {
                    name: f.name,
                    is_dir,
                }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn delete(&self, path: &str) -> Result<()> {
        if let Some(file) = self.resolve_file(path).await? {
            self.client.delete_file(&file.id).await?;
        }
        Ok(())
    }

    async fn quota(&self) -> Result<Option<Quota>> {
        let q = self.client.about_quota().await?;
        Ok(Some(Quota {
            limit: parse_u64(&q.limit),
            usage: parse_u64(&q.usage),
            usage_in_drive: parse_u64(&q.usage_in_drive),
            usage_in_drive_trash: parse_u64(&q.usage_in_drive_trash),
        }))
    }

    async fn put_from_file(&self, path: &str, file: &Path) -> Result<()> {
        let (dir, name) = crate::storage::split_path(path);
        let parent = self.ensure_folder(&dir).await?;
        let size = tokio::fs::metadata(file).await?.len();
        let kind = path_kind(path);
        // Replace any existing content. Content-addressed objects never collide,
        // so this mainly affects mutable metadata files.
        if let Some(existing) = self.client.find_child(&parent, &name).await? {
            self.client.delete_file(&existing.id).await?;
        }
        if size >= self.client.resumable_threshold() {
            self.client
                .upload_resumable(&parent, &name, "application/octet-stream", file, marker(kind))
                .await?;
        } else {
            let data = tokio::fs::read(file).await?;
            self.client
                .upload_new(&parent, &name, "application/octet-stream", &data, marker(kind))
                .await?;
        }
        Ok(())
    }

    async fn get_to_file(&self, path: &str, dest: &Path) -> Result<()> {
        let file = self
            .resolve_file(path)
            .await?
            .ok_or_else(|| Error::MissingObject(path.to_string()))?;
        self.client.download_to_file(&file.id, dest).await
    }
}

fn parse_u64(s: &Option<String>) -> u64 {
    s.as_deref().and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Classify a path so objects can be distinguished from metadata in Drive.
fn path_kind(path: &str) -> &'static str {
    if path.starts_with("objects/") {
        "object"
    } else if path.starts_with("snapshots/") {
        "snapshot"
    } else if path.starts_with("index/") {
        "index"
    } else {
        "meta"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_kinds() {
        assert_eq!(path_kind("objects/ab/cd"), "object");
        assert_eq!(path_kind("snapshots/x.json"), "snapshot");
        assert_eq!(path_kind("index/objects/ab.json"), "index");
        assert_eq!(path_kind("repository.json"), "meta");
    }
}
