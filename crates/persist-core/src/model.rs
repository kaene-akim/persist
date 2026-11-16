//! Core data model: the on-disk repository format and the local configuration
//! types.
//!
//! ## Repository format
//!
//! A Persist repository lives entirely inside the cloud storage backend under a
//! flat, `/`-separated logical namespace:
//!
//! ```text
//! repository.json                 repository metadata (format version, id, name)
//! refs/head.json                  pointer to the most recent snapshot
//! snapshots/<snapshot-id>.json    immutable snapshot manifests
//! objects/<aa>/<sha256>           content-addressable file objects
//! index/objects/<aa>.json         sharded object index (hash -> size)
//! ```
//!
//! The cloud repository is the authoritative source of history. The local SQLite
//! database only caches information to make the interfaces fast; it can always be
//! rebuilt by reading the cloud repository (see `persist clone`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Current repository format version. Bump on incompatible changes.
pub const FORMAT_VERSION: u32 = 1;

/// Version of the snapshot manifest schema (independent of the repo format).
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Repository-level metadata, stored at `repository.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoMeta {
    pub format_version: u32,
    pub repo_id: String,
    pub name: String,
    /// Storage backend identifier, e.g. `google-drive` or `local`.
    pub backend: String,
    pub created_at: DateTime<Utc>,
    /// Free-form, forward compatible extension point.
    #[serde(default)]
    pub extensions: serde_json::Value,
}

impl RepoMeta {
    pub fn new(repo_id: impl Into<String>, name: impl Into<String>, backend: impl Into<String>) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            repo_id: repo_id.into(),
            name: name.into(),
            backend: backend.into(),
            created_at: Utc::now(),
            extensions: serde_json::json!({}),
        }
    }
}

/// The kind of filesystem object represented by an entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    #[default]
    File,
    Directory,
    Symlink,
}

/// A single filesystem entry captured in a snapshot.
///
/// `path` is always a `/`-separated path relative to the backup root, and is
/// validated on read (see [`crate::util::sanitize_rel_path`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    #[serde(default)]
    pub kind: EntryKind,
    pub size: u64,
    /// SHA-256 of the file contents. `None` for directories and symlinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Symlink target (verbatim) for [`EntryKind::Symlink`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    /// Unix permission bits.
    #[serde(default)]
    pub mode: u32,
    /// Last modification time, unix seconds.
    #[serde(default)]
    pub mtime: i64,
}

impl FileEntry {
    /// True when this entry stores content (i.e. has an object behind it).
    pub fn has_content(&self) -> bool {
        self.kind == EntryKind::File && self.hash.is_some()
    }
}

/// One backup root as recorded in a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootState {
    pub root_id: String,
    pub name: String,
    /// Original local path on the machine that created the snapshot. Informative
    /// only; it is never used verbatim when restoring on another machine.
    pub local_path: String,
    /// Destination path inside the repository (logical).
    #[serde(default)]
    pub remote_path: String,
    pub entries: Vec<FileEntry>,
    /// Exclusion patterns that were active when the snapshot was taken, so that
    /// history remains self-describing.
    #[serde(default)]
    pub exclusions: Vec<String>,
}

/// Aggregate statistics about a snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotStats {
    pub file_count: u64,
    pub dir_count: u64,
    pub symlink_count: u64,
    /// Sum of the sizes of all files in the snapshot (the *logical* size).
    pub logical_size: u64,
    /// Number of new objects uploaded while creating this snapshot.
    #[serde(default)]
    pub new_objects: u64,
    /// Bytes of new objects uploaded while creating this snapshot.
    #[serde(default)]
    pub new_bytes: u64,
    /// Files added/modified/deleted relative to the parent snapshot.
    #[serde(default)]
    pub added: u64,
    #[serde(default)]
    pub modified: u64,
    #[serde(default)]
    pub deleted: u64,
}

/// An immutable snapshot manifest, stored at `snapshots/<id>.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub format_version: u32,
    pub snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub roots: Vec<RootState>,
    /// Entries removed relative to the parent snapshot, formatted `root_id:path`.
    #[serde(default)]
    pub deleted: Vec<String>,
    pub stats: SnapshotStats,
}

impl Snapshot {
    /// Every content hash referenced by this snapshot (deduplicated).
    pub fn referenced_hashes(&self) -> std::collections::BTreeSet<String> {
        let mut set = std::collections::BTreeSet::new();
        for root in &self.roots {
            for entry in &root.entries {
                if let Some(h) = &entry.hash {
                    set.insert(h.clone());
                }
            }
        }
        set
    }

    /// Total logical size across all roots.
    pub fn logical_size(&self) -> u64 {
        self.roots
            .iter()
            .flat_map(|r| r.entries.iter())
            .map(|e| e.size)
            .sum()
    }

    /// Validate a snapshot manifest read from (untrusted) storage. Rejects
    /// malformed paths and hashes so that a corrupted or hostile manifest can
    /// never drive a restore outside its destination.
    pub fn validate(&self) -> Result<()> {
        if self.format_version > SNAPSHOT_FORMAT_VERSION {
            return Err(Error::InvalidRepository(format!(
                "snapshot {} uses format {} which is newer than supported ({SNAPSHOT_FORMAT_VERSION})",
                self.snapshot_id, self.format_version
            )));
        }
        if self.snapshot_id.trim().is_empty() {
            return Err(Error::InvalidRepository("snapshot has an empty id".into()));
        }
        for root in &self.roots {
            if root.root_id.trim().is_empty() {
                return Err(Error::InvalidRepository(format!(
                    "snapshot {} has a root with an empty id",
                    self.snapshot_id
                )));
            }
            for entry in &root.entries {
                if crate::util::sanitize_rel_path(&entry.path).is_none() {
                    return Err(Error::InvalidRepository(format!(
                        "snapshot {} contains an unsafe path: {:?}",
                        self.snapshot_id, entry.path
                    )));
                }
                if let Some(hash) = &entry.hash {
                    if !crate::hashing::is_valid_hash(hash) {
                        return Err(Error::InvalidRepository(format!(
                            "snapshot {} contains an invalid object hash for {}",
                            self.snapshot_id, entry.path
                        )));
                    }
                }
                if entry.kind == EntryKind::File && entry.hash.is_none() {
                    return Err(Error::InvalidRepository(format!(
                        "snapshot {} has a file entry without content hash: {}",
                        self.snapshot_id, entry.path
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Pointer to the current head snapshot, stored at `refs/head.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeadRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// A shard of the object index, stored at `index/objects/<aa>.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ObjectIndexShard {
    /// Map of object hash -> size in bytes.
    pub objects: std::collections::BTreeMap<String, u64>,
}

/// How a file changed between two states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Unchanged,
}

/// A single path-level change between two snapshots (or the working tree and a
/// snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub root_id: String,
    pub root_name: String,
    pub path: String,
    pub kind: ChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<FileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<FileEntry>,
}

/// A full set of changes plus aggregate counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeSet {
    pub changes: Vec<FileChange>,
    pub added: u64,
    pub modified: u64,
    pub deleted: u64,
    pub unchanged: u64,
    /// Total logical size of the *new* state for changed entries.
    pub changed_bytes: u64,
}

impl ChangeSet {
    pub fn from_changes(changes: Vec<FileChange>) -> Self {
        let mut set = ChangeSet {
            changes,
            ..Default::default()
        };
        for c in &set.changes {
            match c.kind {
                ChangeKind::Added => {
                    set.added += 1;
                    set.changed_bytes += c.new.as_ref().map(|e| e.size).unwrap_or(0);
                }
                ChangeKind::Modified => {
                    set.modified += 1;
                    set.changed_bytes += c.new.as_ref().map(|e| e.size).unwrap_or(0);
                }
                ChangeKind::Deleted => set.deleted += 1,
                ChangeKind::Unchanged => set.unchanged += 1,
            }
        }
        set
    }
}

/// A locally managed backup directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RootConfig {
    pub root_id: String,
    pub name: String,
    pub local_path: PathBuf,
    /// Logical destination inside the repository.
    pub remote_path: String,
    /// Per-directory exclusion patterns (combined with the global defaults).
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

/// Global, user-editable settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Patterns applied to every root.
    pub default_exclusions: Vec<String>,
    /// Ask for confirmation before starting an upload.
    pub confirm_before_backup: bool,
    /// Re-download and hash objects after upload to prove integrity.
    pub verify_uploads: bool,
    /// Chunk size for resumable uploads, in bytes.
    pub upload_chunk_size: u64,
    /// Follow symbolic links instead of recording them as links.
    pub follow_symlinks: bool,
    /// Whether symlinks are recorded at all.
    pub store_symlinks: bool,
    /// Skip files larger than this many bytes (0 = no limit).
    pub max_file_size: u64,
    /// Number of concurrent uploads.
    pub upload_concurrency: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_exclusions: default_exclusions(),
            confirm_before_backup: true,
            verify_uploads: true,
            upload_chunk_size: 8 * 1024 * 1024,
            follow_symlinks: false,
            store_symlinks: true,
            max_file_size: 0,
            upload_concurrency: 4,
        }
    }
}

/// Sensible default exclusion patterns, inspired by common developer setups.
pub fn default_exclusions() -> Vec<String> {
    [
        ".git/",
        ".svn/",
        ".hg/",
        "node_modules/",
        "__pycache__/",
        ".cache/",
        ".venv/",
        "venv/",
        "target/",
        "dist/",
        "build/",
        ".DS_Store",
        "Thumbs.db",
        "*.tmp",
        "*.temp",
        "*.log",
        "*.swp",
        "*.swo",
        "*~",
        ".Trash/",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Result of a completed backup, returned by the engine and shown in the UI/CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub snapshot_id: String,
    pub parent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub files_backed_up: u64,
    pub logical_size: u64,
    pub new_objects: u64,
    pub new_bytes: u64,
    pub added: u64,
    pub modified: u64,
    pub deleted: u64,
    pub unchanged: u64,
    pub duration_secs: f64,
}

/// A planned backup, produced by analysis and used both for previews and for
/// driving the upload phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPlan {
    pub roots: Vec<PlannedRoot>,
    /// Objects that need to be uploaded: (hash, size, representative local path).
    pub uploads: Vec<PlannedUpload>,
    pub change_set: ChangeSet,
    pub total_files: u64,
    pub total_logical_size: u64,
    pub upload_bytes: u64,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    pub is_initial: bool,
    pub head_snapshot: Option<String>,
}

/// Per-root part of a backup plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedRoot {
    pub root_id: String,
    pub name: String,
    pub local_path: String,
    pub remote_path: String,
    pub state: RootState,
}

/// One object scheduled for upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedUpload {
    pub hash: String,
    pub size: u64,
    pub local_path: PathBuf,
}

/// Storage quota information, either from the provider or the local backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Quota {
    pub limit: u64,
    pub usage: u64,
    #[serde(default)]
    pub usage_in_drive: u64,
    #[serde(default)]
    pub usage_in_drive_trash: u64,
}

impl Quota {
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.usage)
    }
}

/// Persist's own storage footprint within a repository.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoUsage {
    pub object_count: u64,
    /// Actual bytes stored in the repository (objects + metadata).
    pub physical_bytes: u64,
    pub snapshot_count: u64,
    /// Logical size of the latest snapshot (the data currently protected).
    pub logical_bytes: u64,
    /// Sum of the logical sizes of every snapshot ever taken. Because unchanged
    /// files are shared, physical storage is normally far below this number.
    pub total_logical_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_exclusions_non_empty() {
        assert!(default_exclusions().contains(&"node_modules/".to_string()));
    }

    #[test]
    fn snapshot_referenced_hashes_dedup() {
        let e = |p: &str, h: &str| FileEntry {
            path: p.into(),
            kind: EntryKind::File,
            size: 1,
            hash: Some(h.into()),
            link_target: None,
            mode: 0o644,
            mtime: 0,
        };
        let snap = Snapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            snapshot_id: "s1".into(),
            parent: None,
            created_at: Utc::now(),
            message: None,
            roots: vec![RootState {
                root_id: "r".into(),
                name: "r".into(),
                local_path: "/tmp".into(),
                remote_path: "r".into(),
                entries: vec![e("a", "aa"), e("b", "aa"), e("c", "bb")],
                exclusions: vec![],
            }],
            deleted: vec![],
            stats: SnapshotStats::default(),
        };
        assert_eq!(snap.referenced_hashes().len(), 2);
    }
}
