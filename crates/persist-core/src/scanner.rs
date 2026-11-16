//! Filesystem scanner.
//!
//! Walks a backup root, applies exclusion rules, and produces the list of
//! [`FileEntry`] values that describe the current state of the directory.
//!
//! Design notes:
//! * Directory traversal is blocking (`walkdir`) and runs on a blocking task.
//! * File hashing is asynchronous and bounded-concurrency, because hashing is
//!   the expensive part and is I/O bound.
//! * File identity is established by content hash, never by timestamp or size
//!   alone. Timestamps are recorded but only used as a hint for skipping work in
//!   the local cache (see the engine), never as proof of equality.

use crate::error::{Error, Result};
use crate::exclude::ExcludeRules;
use crate::hashing;
use crate::model::{EntryKind, FileEntry};
use crate::util::to_rel_string;
use futures::stream::{self, StreamExt};
use std::path::{Path, PathBuf};

/// Options controlling a scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub exclusions: ExcludeRules,
    pub follow_symlinks: bool,
    pub store_symlinks: bool,
    /// Skip files larger than this many bytes (0 = unlimited).
    pub max_file_size: u64,
    /// Number of files to hash concurrently.
    pub concurrency: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            exclusions: ExcludeRules::default(),
            follow_symlinks: false,
            store_symlinks: true,
            max_file_size: 0,
            concurrency: 8,
        }
    }
}

/// A non-fatal problem encountered while scanning.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub message: String,
}

/// Outcome of scanning a directory.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub entries: Vec<FileEntry>,
    pub excluded_files: u64,
    pub excluded_bytes: u64,
    pub skipped_large_files: u64,
    pub skipped_large_bytes: u64,
    pub issues: Vec<ScanIssue>,
}

/// Intermediate entry before hashing.
#[derive(Debug, Clone)]
struct RawEntry {
    rel: String,
    kind: EntryKind,
    size: u64,
    mode: u32,
    mtime: i64,
    link_target: Option<String>,
}

/// Scan `root` according to `opts`.
pub async fn scan(root: &Path, opts: &ScanOptions) -> Result<ScanResult> {
    let root = root.to_path_buf();
    if !root.exists() {
        return Err(Error::DirectoryMissing(root));
    }
    if !root.is_dir() {
        return Err(Error::Config(format!(
            "{} is not a directory",
            root.display()
        )));
    }

    let opts_bg = opts.clone();
    let root_bg = root.clone();
    // Phase 1: walk the tree (blocking).
    let (raw, mut result) = tokio::task::spawn_blocking(move || walk(&root_bg, &opts_bg))
        .await
        .map_err(|e| Error::Message(format!("scanner task panicked: {e}")))??;

    // Phase 2: hash files with bounded concurrency.
    let concurrency = opts.concurrency.max(1);
    let root_hash = root.clone();
    let hashed: Vec<Result<FileEntry>> = stream::iter(raw.into_iter().map(|r| {
        let root = root_hash.clone();
        async move { hash_entry(&root, r).await }
    }))
    .buffered(concurrency)
    .collect()
    .await;

    for h in hashed {
        match h {
            Ok(entry) => result.entries.push(entry),
            Err(e) => result.issues.push(ScanIssue {
                path: PathBuf::new(),
                message: e.to_string(),
            }),
        }
    }

    result.entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

/// Blocking directory walk.
fn walk(root: &Path, opts: &ScanOptions) -> Result<(Vec<RawEntry>, ScanResult)> {
    let mut raw = Vec::new();
    let mut result = ScanResult::default();

    let walker = walkdir::WalkDir::new(root)
        .follow_links(opts.follow_symlinks)
        .same_file_system(false);
    for entry in walker.into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                result.issues.push(ScanIssue {
                    path: err.path().map(|p| p.to_path_buf()).unwrap_or_default(),
                    message: err.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = to_rel_string(path.strip_prefix(root).unwrap_or(path));
        if rel.is_empty() {
            continue;
        }
        let file_type = entry.file_type();
        let is_dir = file_type.is_dir();

        if opts.exclusions.is_excluded(&rel, is_dir) {
            if !is_dir {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                result.excluded_files += 1;
                result.excluded_bytes += size;
            }
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                result.issues.push(ScanIssue {
                    path: path.to_path_buf(),
                    message: err.to_string(),
                });
                continue;
            }
        };

        if file_type.is_symlink() && !opts.follow_symlinks {
            if !opts.store_symlinks {
                continue;
            }
            let target = std::fs::read_link(path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            raw.push(RawEntry {
                rel,
                kind: EntryKind::Symlink,
                size: 0,
                mode: mode_of(&meta),
                mtime: mtime_of(&meta),
                link_target: Some(target),
            });
            continue;
        }

        if is_dir {
            raw.push(RawEntry {
                rel,
                kind: EntryKind::Directory,
                size: 0,
                mode: mode_of(&meta),
                mtime: mtime_of(&meta),
                link_target: None,
            });
            continue;
        }

        if file_type.is_file() {
            if opts.max_file_size > 0 && meta.len() > opts.max_file_size {
                result.skipped_large_files += 1;
                result.skipped_large_bytes += meta.len();
                continue;
            }
            raw.push(RawEntry {
                rel,
                kind: EntryKind::File,
                size: meta.len(),
                mode: mode_of(&meta),
                mtime: mtime_of(&meta),
                link_target: None,
            });
            continue;
        }
        // Sockets, FIFOs, devices: recorded as metadata only, never content.
        raw.push(RawEntry {
            rel,
            kind: EntryKind::Symlink,
            size: 0,
            mode: mode_of(&meta),
            mtime: mtime_of(&meta),
            link_target: None,
        });
    }

    Ok((raw, result))
}

/// Hash a raw entry, producing a final [`FileEntry`].
async fn hash_entry(root: &Path, raw: RawEntry) -> Result<FileEntry> {
    if raw.kind != EntryKind::File {
        return Ok(FileEntry {
            path: raw.rel,
            kind: raw.kind,
            size: raw.size,
            hash: None,
            link_target: raw.link_target,
            mode: raw.mode,
            mtime: raw.mtime,
        });
    }
    let full = root.join(raw.rel.split('/').collect::<PathBuf>());
    let hash = hashing::hash_file(&full).await?;
    Ok(FileEntry {
        path: raw.rel,
        kind: EntryKind::File,
        size: raw.size,
        hash: Some(hash),
        link_target: None,
        mode: raw.mode,
        mtime: raw.mtime,
    })
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

#[cfg(not(unix))]
fn mode_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), b"aaa").unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"bbb").unwrap();
        fs::create_dir_all(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/x.js"), b"x").unwrap();
        fs::write(dir.path().join("skip.log"), b"log").unwrap();
        dir
    }

    #[tokio::test]
    async fn scan_applies_exclusions_and_hashes() {
        let dir = setup();
        let opts = ScanOptions {
            exclusions: ExcludeRules::compile(&["node_modules/".into(), "*.log".into()]).unwrap(),
            ..Default::default()
        };
        let res = scan(dir.path(), &opts).await.unwrap();
        let files: Vec<_> = res
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::File)
            .map(|e| e.path.clone())
            .collect();
        assert!(files.contains(&"a.txt".to_string()));
        assert!(files.contains(&"sub/b.txt".to_string()));
        assert!(!files.iter().any(|p| p.contains("node_modules")));
        assert!(!files.iter().any(|p| p.ends_with(".log")));
        assert_eq!(res.excluded_files, 2);
        // hashes present
        let a = res.entries.iter().find(|e| e.path == "a.txt").unwrap();
        assert_eq!(a.hash.as_deref(), Some(hashing::hash_bytes(b"aaa").as_str()));
    }

    #[tokio::test]
    async fn missing_directory_errors() {
        let opts = ScanOptions::default();
        let err = scan(Path::new("/nonexistent/xyz"), &opts).await;
        assert!(matches!(err, Err(Error::DirectoryMissing(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_recorded_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("target.txt"), b"hi").unwrap();
        std::os::unix::fs::symlink("target.txt", dir.path().join("link")).unwrap();
        let res = scan(dir.path(), &ScanOptions::default()).await.unwrap();
        let link = res.entries.iter().find(|e| e.path == "link").unwrap();
        assert_eq!(link.kind, EntryKind::Symlink);
        assert_eq!(link.link_target.as_deref(), Some("target.txt"));
    }
}
