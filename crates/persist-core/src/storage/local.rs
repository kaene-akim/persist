//! A [`Storage`] backend backed by the local filesystem.
//!
//! This is used for two purposes:
//! * as a fully featured "local repository" target for offline use and tests;
//! * as the reference implementation of the storage contract.

use super::{DirEntry, Storage};
use crate::error::{Error, Result};
use crate::model::Quota;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Filesystem-backed storage rooted at a directory.
#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn full(&self, path: &str) -> PathBuf {
        let mut p = self.root.clone();
        for part in path.split('/').filter(|s| !s.is_empty()) {
            p.push(part);
        }
        p
    }

    /// Directory usage, used to report a "quota" for local repositories.
    pub fn used_bytes(&self) -> u64 {
        walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }
}

#[async_trait]
impl Storage for LocalStorage {
    fn backend_name(&self) -> &'static str {
        "local"
    }

    async fn put(&self, path: &str, data: &[u8]) -> Result<()> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Write to a temporary file then rename, so readers never observe a
        // partially written object.
        let tmp = full.with_extension("tmp-write");
        tokio::fs::write(&tmp, data).await?;
        tokio::fs::rename(&tmp, &full).await?;
        Ok(())
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        let full = self.full(path);
        match tokio::fs::read(&full).await {
            Ok(v) => Ok(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::MissingObject(path.to_string())),
            Err(e) => Err(e.into()),
        }
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(tokio::fs::try_exists(self.full(path)).await.unwrap_or(false))
    }

    async fn list(&self, dir: &str) -> Result<Vec<DirEntry>> {
        let full = self.full(dir);
        let mut out = Vec::new();
        let mut rd = match tokio::fs::read_dir(&full).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".tmp-write") {
                continue;
            }
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            out.push(DirEntry { name, is_dir });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let full = self.full(path);
        match tokio::fs::remove_file(&full).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn quota(&self) -> Result<Option<Quota>> {
        // Local repositories have no external quota; report usage only.
        Ok(Some(Quota {
            limit: 0,
            usage: self.used_bytes(),
            usage_in_drive: 0,
            usage_in_drive_trash: 0,
        }))
    }

    async fn put_from_file(&self, path: &str, file: &Path) -> Result<()> {
        let full = self.full(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = full.with_extension("tmp-write");
        tokio::fs::copy(file, &tmp).await?;
        tokio::fs::rename(&tmp, &full).await?;
        Ok(())
    }

    async fn get_to_file(&self, path: &str, dest: &Path) -> Result<()> {
        let full = self.full(path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&full, dest).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let s = LocalStorage::new(dir.path());
        s.put("objects/aa/bb", b"hello").await.unwrap();
        assert!(s.exists("objects/aa/bb").await.unwrap());
        assert_eq!(s.get("objects/aa/bb").await.unwrap(), b"hello");
        let l = s.list("objects/aa").await.unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "bb");
        s.delete("objects/aa/bb").await.unwrap();
        assert!(!s.exists("objects/aa/bb").await.unwrap());
        assert!(matches!(
            s.get("objects/aa/bb").await,
            Err(Error::MissingObject(_))
        ));
    }
}
