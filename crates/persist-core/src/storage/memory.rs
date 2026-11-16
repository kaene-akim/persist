//! In-memory [`Storage`] backend, used by unit tests and by `--dry-run` style
//! simulations that must never touch the network.

use super::{DirEntry, Storage};
use crate::error::{Error, Result};
use crate::model::Quota;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A simple, thread-safe map from logical path to bytes.
#[derive(Clone, Default)]
pub struct MemoryStorage {
    inner: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
    /// When set, every Nth write fails, to exercise retry/recovery logic.
    fail_every: Arc<RwLock<Option<u64>>>,
    writes: Arc<RwLock<u64>>,
}

impl std::fmt::Debug for MemoryStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStorage").finish_non_exhaustive()
    }
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of successful writes performed so far.
    pub fn write_count(&self) -> u64 {
        *self.writes.read()
    }

    /// Configure the backend to fail every `n`-th write (for tests).
    pub fn fail_every(&self, n: u64) {
        *self.fail_every.write() = Some(n);
    }

    /// All paths currently stored.
    pub fn paths(&self) -> Vec<String> {
        self.inner.read().keys().cloned().collect()
    }

    pub fn total_bytes(&self) -> u64 {
        self.inner.read().values().map(|v| v.len() as u64).sum()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    fn backend_name(&self) -> &'static str {
        "memory"
    }

    async fn put(&self, path: &str, data: &[u8]) -> Result<()> {
        {
            let mut w = self.writes.write();
            *w += 1;
            if let Some(n) = *self.fail_every.read() {
                if n > 0 && *w % n == 0 {
                    return Err(Error::Network("injected transient failure".into()));
                }
            }
        }
        self.inner.write().insert(path.to_string(), data.to_vec());
        Ok(())
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        self.inner
            .read()
            .get(path)
            .cloned()
            .ok_or_else(|| Error::MissingObject(path.to_string()))
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.inner.read().contains_key(path))
    }

    async fn list(&self, dir: &str) -> Result<Vec<DirEntry>> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut names: BTreeMap<String, bool> = BTreeMap::new();
        for key in self.inner.read().keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if rest.is_empty() {
                    continue;
                }
                match rest.split_once('/') {
                    Some((first, _)) => {
                        names.insert(first.to_string(), true);
                    }
                    None => {
                        names.insert(rest.to_string(), false);
                    }
                }
            }
        }
        Ok(names
            .into_iter()
            .map(|(name, is_dir)| DirEntry { name, is_dir })
            .collect())
    }

    async fn delete(&self, path: &str) -> Result<()> {
        self.inner.write().remove(path);
        Ok(())
    }

    async fn quota(&self) -> Result<Option<Quota>> {
        Ok(Some(Quota {
            limit: 0,
            usage: self.total_bytes(),
            usage_in_drive: 0,
            usage_in_drive_trash: 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listing_reports_dirs() {
        let s = MemoryStorage::new();
        s.put("a/b/c", b"1").await.unwrap();
        s.put("a/d", b"2").await.unwrap();
        let l = s.list("a").await.unwrap();
        assert!(l.iter().any(|e| e.name == "b" && e.is_dir));
        assert!(l.iter().any(|e| e.name == "d" && !e.is_dir));
    }
}
