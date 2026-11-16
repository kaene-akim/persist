//! The repository: the bridge between the backup engine and a [`Storage`]
//! backend.
//!
//! The repository owns the cloud-side format (metadata, head ref, snapshots,
//! content objects and the sharded object index). It is deliberately the only
//! component that knows the storage layout, so the engine can stay provider
//! agnostic.

pub mod layout;

use crate::error::{Error, Result};
use crate::hashing;
use crate::model::{
    HeadRef, ObjectIndexShard, RepoMeta, RepoUsage, Snapshot, FORMAT_VERSION,
};
use crate::storage::{DirEntry, StorageRef};
use crate::util;
use chrono::Utc;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Number of new objects to accumulate before flushing the index. Bounds the
/// amount of re-uploading after an interruption while keeping API traffic low.
const INDEX_FLUSH_INTERVAL: usize = 128;

#[derive(Default)]
struct IndexState {
    shards: HashMap<String, ObjectIndexShard>,
    dirty: HashSet<String>,
    /// Objects added since the last flush.
    pending: usize,
}

/// A Persist repository backed by a [`Storage`] implementation.
pub struct Repository {
    storage: StorageRef,
    meta: RepoMeta,
    index: Mutex<IndexState>,
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repository")
            .field("repo_id", &self.meta.repo_id)
            .field("backend", &self.meta.backend)
            .finish_non_exhaustive()
    }
}

impl Repository {
    /// Initialise a brand new repository in `storage`.
    pub async fn init(
        storage: StorageRef,
        name: &str,
        repo_id: Option<String>,
    ) -> Result<Self> {
        if storage.exists(layout::REPO_META).await? {
            let existing = Self::open(storage.clone()).await?;
            return Err(Error::Config(format!(
                "a Persist repository ({}) already exists at this location",
                existing.meta.repo_id
            )));
        }
        let repo_id = repo_id.unwrap_or_else(util::new_id);
        let meta = RepoMeta::new(repo_id, name, storage.backend_name());
        storage
            .put(layout::REPO_META, &serde_json::to_vec_pretty(&meta)?)
            .await?;
        let head = HeadRef {
            snapshot: None,
            updated_at: Utc::now(),
        };
        storage
            .put(layout::HEAD_REF, &serde_json::to_vec_pretty(&head)?)
            .await?;
        Ok(Self {
            storage,
            meta,
            index: Mutex::new(IndexState::default()),
        })
    }

    /// Open an existing repository. Fails with a clear error if the location is
    /// not a Persist repository.
    pub async fn open(storage: StorageRef) -> Result<Self> {
        let raw = storage.get_opt(layout::REPO_META).await?.ok_or_else(|| {
            Error::RepositoryNotFound(
                "no repository.json found at the configured location".to_string(),
            )
        })?;
        let meta: RepoMeta = serde_json::from_slice(&raw)
            .map_err(|e| Error::InvalidRepository(format!("repository.json is malformed: {e}")))?;
        if meta.format_version > FORMAT_VERSION {
            return Err(Error::InvalidRepository(format!(
                "repository format {} is newer than this build supports ({FORMAT_VERSION}); \
                 please update Persist",
                meta.format_version
            )));
        }
        Ok(Self {
            storage,
            meta,
            index: Mutex::new(IndexState::default()),
        })
    }

    pub fn meta(&self) -> &RepoMeta {
        &self.meta
    }

    pub fn storage(&self) -> &StorageRef {
        &self.storage
    }

    pub fn repo_id(&self) -> &str {
        &self.meta.repo_id
    }

    // -- Head ref -----------------------------------------------------------

    /// The current head snapshot id, if any.
    pub async fn head(&self) -> Result<Option<String>> {
        match self.storage.get_opt(layout::HEAD_REF).await? {
            Some(raw) => {
                let head: HeadRef = serde_json::from_slice(&raw).map_err(|e| {
                    Error::InvalidRepository(format!("refs/head.json is malformed: {e}"))
                })?;
                Ok(head.snapshot)
            }
            None => Ok(None),
        }
    }

    /// Update the head pointer. Called last, after a snapshot is committed.
    pub async fn set_head(&self, snapshot: Option<String>) -> Result<()> {
        let head = HeadRef {
            snapshot,
            updated_at: Utc::now(),
        };
        self.storage
            .put(layout::HEAD_REF, &serde_json::to_vec_pretty(&head)?)
            .await
    }

    // -- Snapshots ----------------------------------------------------------

    /// List snapshot ids, sorted oldest first (ids are time ordered).
    pub async fn list_snapshot_ids(&self) -> Result<Vec<String>> {
        let entries = self.storage.list(layout::SNAPSHOTS_DIR).await?;
        let mut ids: Vec<String> = entries
            .into_iter()
            .filter(|e| !e.is_dir)
            .filter_map(|e| e.name.strip_suffix(".json").map(|s| s.to_string()))
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// Load a snapshot manifest by id.
    pub async fn load_snapshot(&self, id: &str) -> Result<Snapshot> {
        let raw = self
            .storage
            .get_opt(&layout::snapshot_path(id))
            .await?
            .ok_or_else(|| Error::SnapshotNotFound(id.to_string()))?;
        let snap: Snapshot = serde_json::from_slice(&raw).map_err(|e| {
            Error::InvalidRepository(format!("snapshot {id} is malformed: {e}"))
        })?;
        snap.validate()?;
        Ok(snap)
    }

    /// Load a snapshot, or the head snapshot when `id` is `None`.
    pub async fn load_snapshot_or_head(&self, id: Option<&str>) -> Result<Snapshot> {
        let id = match id {
            Some(id) => id.to_string(),
            None => self
                .head()
                .await?
                .ok_or_else(|| Error::SnapshotNotFound("no snapshots exist yet".to_string()))?,
        };
        self.load_snapshot(&id).await
    }

    /// Write a snapshot manifest. Manifests are immutable: writing over an
    /// existing snapshot id is refused.
    pub async fn save_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        snapshot.validate()?;
        let path = layout::snapshot_path(&snapshot.snapshot_id);
        if self.storage.exists(&path).await? {
            return Err(Error::InvalidRepository(format!(
                "snapshot {} already exists and is immutable",
                snapshot.snapshot_id
            )));
        }
        let data = serde_json::to_vec_pretty(snapshot)?;
        self.storage.put(&path, &data).await
    }

    /// Delete a snapshot manifest. Used by maintenance operations; the head
    /// pointer is not touched.
    pub async fn delete_snapshot(&self, id: &str) -> Result<()> {
        self.storage.delete(&layout::snapshot_path(id)).await
    }

    // -- Object index -------------------------------------------------------

    /// Load an index shard, consulting the cache first.
    async fn load_shard(&self, shard: &str) -> Result<ObjectIndexShard> {
        {
            let state = self.index.lock();
            if let Some(s) = state.shards.get(shard) {
                return Ok(s.clone());
            }
        }
        let path = layout::index_shard_path(shard);
        let shard_data = match self.storage.get_opt(&path).await? {
            Some(raw) => serde_json::from_slice(&raw).map_err(|e| {
                Error::InvalidRepository(format!("object index shard {shard} is malformed: {e}"))
            })?,
            None => ObjectIndexShard::default(),
        };
        self.index
            .lock()
            .shards
            .insert(shard.to_string(), shard_data.clone());
        Ok(shard_data)
    }

    /// Whether an object with the given hash already exists in the repository.
    pub async fn object_exists(&self, hash: &str) -> Result<bool> {
        let shard = layout::shard_of(hash);
        let s = self.load_shard(&shard).await?;
        Ok(s.objects.contains_key(hash))
    }

    /// Record an object in the index (marking the shard dirty).
    pub async fn index_object(&self, hash: &str, size: u64) -> Result<()> {
        let shard = layout::shard_of(hash);
        let _ = self.load_shard(&shard).await?;
        // Scope the guard so it is never held across the `flush_index().await`
        // below; `parking_lot` guards are `!Send` and would make the whole
        // future non-`Send`, which the Tauri command layer requires.
        let flush = {
            let mut state = self.index.lock();
            let entry = state.shards.entry(shard.clone()).or_default();
            let inserted = entry.objects.insert(hash.to_string(), size).is_none();
            state.dirty.insert(shard.clone());
            if inserted {
                state.pending += 1;
            }
            state.pending >= INDEX_FLUSH_INTERVAL
        };
        if flush {
            self.flush_index().await?;
        }
        Ok(())
    }

    /// Persist all dirty index shards to storage.
    pub async fn flush_index(&self) -> Result<()> {
        let to_write: Vec<(String, ObjectIndexShard)> = {
            let state = self.index.lock();
            state
                .dirty
                .iter()
                .filter_map(|shard| {
                    state
                        .shards
                        .get(shard)
                        .map(|s| (shard.clone(), s.clone()))
                })
                .collect()
        };
        for (shard, data) in to_write {
            let path = layout::index_shard_path(&shard);
            self.storage.put(&path, &serde_json::to_vec(&data)?).await?;
            {
                let mut state = self.index.lock();
                state.dirty.remove(&shard);
                state.pending = 0;
            }
        }
        Ok(())
    }

    /// Rebuild the entire object index from scratch by scanning all snapshot
    /// manifests. This is the authoritative recovery path used by `clone` and
    /// `verify` when the index is missing or suspect.
    pub async fn rebuild_index(&self) -> Result<u64> {
        let ids = self.list_snapshot_ids().await?;
        let mut shards: HashMap<String, ObjectIndexShard> = HashMap::new();
        for id in &ids {
            let snap = self.load_snapshot(id).await?;
            for root in &snap.roots {
                for entry in &root.entries {
                    if let Some(hash) = &entry.hash {
                        let shard = layout::shard_of(hash);
                        shards
                            .entry(shard)
                            .or_default()
                            .objects
                            .insert(hash.clone(), entry.size);
                    }
                }
            }
        }
        let mut count = 0u64;
        for (shard, data) in &shards {
            count += data.objects.len() as u64;
            self.storage
                .put(&layout::index_shard_path(shard), &serde_json::to_vec(data)?)
                .await?;
        }
        // Reset the in-memory cache to the rebuilt state.
        {
            let mut state = self.index.lock();
            state.shards = shards;
            state.dirty.clear();
            state.pending = 0;
        }
        Ok(count)
    }

    /// All known object hashes and sizes.
    pub async fn all_objects(&self) -> Result<HashMap<String, u64>> {
        let entries: Vec<DirEntry> = self.storage.list(layout::INDEX_DIR).await?;
        let mut out = HashMap::new();
        for e in entries {
            if e.is_dir {
                continue;
            }
            let Some(shard) = e.name.strip_suffix(".json") else {
                continue;
            };
            let s = self.load_shard(shard).await?;
            out.extend(s.objects);
        }
        Ok(out)
    }

    // -- Objects ------------------------------------------------------------

    /// Upload an object from a local file, skipping it if it already exists.
    /// Returns `true` when an upload actually happened.
    pub async fn put_object_from_file(&self, hash: &str, size: u64, file: &Path) -> Result<bool> {
        if !hashing::is_valid_hash(hash) {
            return Err(Error::Config(format!("invalid object hash: {hash}")));
        }
        if self.object_exists(hash).await? {
            return Ok(false);
        }
        let path = layout::object_path(hash);
        self.storage.put_from_file(&path, file).await?;
        self.index_object(hash, size).await?;
        Ok(true)
    }

    /// Upload an object from memory.
    pub async fn put_object(&self, hash: &str, data: &[u8]) -> Result<bool> {
        if !hashing::is_valid_hash(hash) {
            return Err(Error::Config(format!("invalid object hash: {hash}")));
        }
        if self.object_exists(hash).await? {
            return Ok(false);
        }
        self.storage.put(&layout::object_path(hash), data).await?;
        self.index_object(hash, data.len() as u64).await?;
        Ok(true)
    }

    /// Download an object to a local file, verifying its hash.
    pub async fn get_object_to_file(&self, hash: &str, dest: &Path) -> Result<()> {
        let path = layout::object_path(hash);
        if !self.storage.exists(&path).await? {
            return Err(Error::MissingObject(hash.to_string()));
        }
        self.storage.get_to_file(&path, dest).await?;
        let actual = hashing::hash_file(dest).await?;
        if actual != hash {
            // Do not leave a corrupt file behind.
            let _ = tokio::fs::remove_file(dest).await;
            return Err(Error::CorruptObject {
                hash: hash.to_string(),
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(())
    }

    /// Download an object into memory, verifying its hash.
    pub async fn get_object(&self, hash: &str) -> Result<Vec<u8>> {
        let data = self.storage.get(&layout::object_path(hash)).await?;
        let actual = hashing::hash_bytes(&data);
        if actual != hash {
            return Err(Error::CorruptObject {
                hash: hash.to_string(),
                expected: hash.to_string(),
                actual,
            });
        }
        Ok(data)
    }

    // -- Usage --------------------------------------------------------------

    /// Compute repository usage statistics.
    pub async fn usage(&self) -> Result<RepoUsage> {
        let objects = self.all_objects().await?;
        let physical: u64 = objects.values().sum();
        let ids = self.list_snapshot_ids().await?;
        let mut head_logical = 0;
        let mut total_logical = 0;
        for id in &ids {
            if let Ok(snap) = self.load_snapshot(id).await {
                let l = snap.logical_size();
                total_logical += l;
            }
        }
        if let Some(head) = self.head().await? {
            if let Ok(snap) = self.load_snapshot(&head).await {
                head_logical = snap.logical_size();
            }
        }
        Ok(RepoUsage {
            object_count: objects.len() as u64,
            physical_bytes: physical,
            snapshot_count: ids.len() as u64,
            logical_bytes: head_logical,
            total_logical_bytes: total_logical,
        })
    }
}
