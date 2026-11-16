//! Storage abstraction.
//!
//! The backup engine never talks to Google Drive (or any other provider)
//! directly. Instead it operates on a [`Storage`] implementation that exposes a
//! flat, `/`-separated logical namespace. This keeps the engine provider
//! agnostic: adding S3, Backblaze or WebDAV later means implementing this trait,
//! not rewriting the engine.
//!
//! Implementations must be safe for concurrent use (`Send + Sync`) and must be
//! idempotent for `put`: writing the same path twice with the same bytes is
//! always acceptable.

pub mod local;
pub mod memory;

use crate::error::{Error, Result};
use crate::model::Quota;
use async_trait::async_trait;
use std::path::Path;

/// An entry returned by [`Storage::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

/// A blob store with a hierarchical logical namespace.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Short identifier of the backend, e.g. `google-drive`.
    fn backend_name(&self) -> &'static str;

    /// Write `data` at `path`, replacing any existing content.
    async fn put(&self, path: &str, data: &[u8]) -> Result<()>;

    /// Read the full contents at `path`.
    async fn get(&self, path: &str) -> Result<Vec<u8>>;

    /// Read the contents at `path`, returning `None` if it does not exist.
    async fn get_opt(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self.get(path).await {
            Ok(v) => Ok(Some(v)),
            Err(Error::MissingObject(_)) | Err(Error::SnapshotNotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Whether `path` exists.
    async fn exists(&self, path: &str) -> Result<bool>;

    /// List the direct children of the directory `dir` (empty string = root).
    async fn list(&self, dir: &str) -> Result<Vec<DirEntry>>;

    /// Delete the object at `path`. Deleting a missing object is not an error.
    async fn delete(&self, path: &str) -> Result<()>;

    /// Report the provider quota, when the provider exposes one.
    async fn quota(&self) -> Result<Option<Quota>> {
        Ok(None)
    }

    /// Upload a file from the local filesystem. Implementations that support
    /// resumable/streaming uploads should override this to avoid buffering the
    /// whole file in memory.
    async fn put_from_file(&self, path: &str, file: &Path) -> Result<()> {
        let data = tokio::fs::read(file).await?;
        self.put(path, &data).await
    }

    /// Download an object directly to a local file.
    async fn get_to_file(&self, path: &str, dest: &Path) -> Result<()> {
        let data = self.get(path).await?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(dest, data).await?;
        Ok(())
    }
}

/// A reference-counted storage backend.
pub type StorageRef = std::sync::Arc<dyn Storage>;

/// Split a logical path into its final component (used by backends that need a
/// file name distinct from its parent folder).
pub(crate) fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_string(), name.to_string()),
        None => (String::new(), path.to_string()),
    }
}
