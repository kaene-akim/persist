//! Restoration.
//!
//! Restoration is the most safety-critical operation in Persist, so it is
//! designed around three rules:
//!
//! 1. **Never silently overwrite.** The default policy is to refuse.
//! 2. **Never escape the destination.** Every path is sanitised and re-checked
//!    for symlink ancestors before anything is written.
//! 3. **Never leave a half-written file.** Objects are downloaded to a temporary
//!    file in the destination directory and atomically renamed into place.

use super::Progress;
use crate::error::{Error, Result};
use crate::model::{EntryKind, FileEntry, Snapshot};
use crate::repo::Repository;
use crate::util::{safe_join, sanitize_rel_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What to do when a file already exists at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverwritePolicy {
    /// Abort the whole operation (the safe default).
    #[default]
    Refuse,
    /// Replace the existing file.
    Overwrite,
    /// Leave the existing file untouched.
    Skip,
    /// Write alongside the existing file with a ` (restored)` suffix.
    Rename,
}

/// Options controlling a restore.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Snapshot to restore; `None` means the head snapshot.
    pub snapshot_id: Option<String>,
    /// Restrict the restore to a single managed root.
    pub root_id: Option<String>,
    /// Restrict the restore to specific paths (relative to the selected root).
    /// Empty means "everything in the selected root(s)".
    pub paths: Vec<String>,
    /// Where to write. `None` restores to the original locations recorded in
    /// the snapshot.
    pub destination: Option<PathBuf>,
    pub overwrite: OverwritePolicy,
    /// Analyse only; write nothing.
    pub dry_run: bool,
    /// Restore permission bits and modification times.
    pub preserve_metadata: bool,
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            snapshot_id: None,
            root_id: None,
            paths: Vec::new(),
            destination: None,
            overwrite: OverwritePolicy::Refuse,
            dry_run: false,
            preserve_metadata: true,
        }
    }
}

/// One item scheduled for restoration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreItem {
    pub root_id: String,
    pub root_name: String,
    /// Path relative to the backup root.
    pub rel_path: String,
    pub kind: EntryKind,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    /// Absolute path this item will be written to.
    pub dest_path: PathBuf,
    /// True when something already exists at `dest_path`.
    pub conflict: bool,
    pub mode: u32,
    pub mtime: i64,
}

/// A planned restore, used for previews and for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    pub snapshot_id: String,
    pub items: Vec<RestoreItem>,
    pub files: u64,
    pub directories: u64,
    pub symlinks: u64,
    pub total_bytes: u64,
    pub conflicts: u64,
}

/// Summary returned after a restore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReport {
    pub snapshot_id: String,
    pub files_restored: u64,
    pub directories_created: u64,
    pub symlinks_restored: u64,
    pub bytes_restored: u64,
    pub skipped: u64,
    pub overwritten: u64,
    pub renamed: u64,
    pub conflicts: u64,
}

/// Work out exactly what a restore would do, without touching the filesystem
/// (apart from reading existing paths to detect conflicts).
pub async fn plan_restore(
    repo: &Repository,
    opts: &RestoreOptions,
) -> Result<(Snapshot, RestorePlan)> {
    let snapshot = repo.load_snapshot_or_head(opts.snapshot_id.as_deref()).await?;

    let selected_roots: Vec<&crate::model::RootState> = snapshot
        .roots
        .iter()
        .filter(|r| opts.root_id.as_ref().map(|id| id == &r.root_id).unwrap_or(true))
        .collect();

    if selected_roots.is_empty() {
        return Err(Error::RootNotFound(
            opts.root_id
                .clone()
                .unwrap_or_else(|| "<all>".to_string()),
        ));
    }

    let path_filter: Vec<String> = opts
        .paths
        .iter()
        .filter_map(|p| sanitize_rel_path(p))
        .collect();

    let mut items = Vec::new();
    for root in &selected_roots {
        let base = restore_base(root, opts);
        for entry in &root.entries {
            if !path_filter.is_empty() && !path_matches(&entry.path, &path_filter) {
                continue;
            }
            let dest = resolve_dest(&base, root, entry, opts, &path_filter);
            let Some(dest) = dest else { continue };
            let conflict = entry.kind != EntryKind::Directory && dest.exists();
            items.push(RestoreItem {
                root_id: root.root_id.clone(),
                root_name: root.name.clone(),
                rel_path: entry.path.clone(),
                kind: entry.kind,
                size: entry.size,
                hash: entry.hash.clone(),
                link_target: entry.link_target.clone(),
                dest_path: dest,
                conflict,
                mode: entry.mode,
                mtime: entry.mtime,
            });
        }
    }

    // Directories first (shallow to deep), then files, so parents exist.
    items.sort_by(|a, b| {
        let depth = |p: &str| p.matches('/').count();
        (a.kind != EntryKind::Directory, depth(&a.rel_path), &a.dest_path).cmp(&(
            b.kind != EntryKind::Directory,
            depth(&b.rel_path),
            &b.dest_path,
        ))
    });

    let plan = RestorePlan {
        snapshot_id: snapshot.snapshot_id.clone(),
        files: items.iter().filter(|i| i.kind == EntryKind::File).count() as u64,
        directories: items.iter().filter(|i| i.kind == EntryKind::Directory).count() as u64,
        symlinks: items.iter().filter(|i| i.kind == EntryKind::Symlink).count() as u64,
        total_bytes: items.iter().map(|i| i.size).sum(),
        conflicts: items.iter().filter(|i| i.conflict).count() as u64,
        items,
    };
    Ok((snapshot, plan))
}

/// Execute a previously planned restore.
pub async fn restore(
    repo: &Repository,
    plan: &RestorePlan,
    opts: &RestoreOptions,
    progress: &dyn Progress,
) -> Result<RestoreReport> {
    if opts.overwrite == OverwritePolicy::Refuse && plan.conflicts > 0 {
        let first = plan
            .items
            .iter()
            .find(|i| i.conflict)
            .map(|i| i.dest_path.display().to_string())
            .unwrap_or_default();
        return Err(Error::RefuseOverwrite(PathBuf::from(format!(
            "{first} (and {} other path(s)) already exist; choose --overwrite, --skip or --rename",
            plan.conflicts.saturating_sub(1)
        ))));
    }

    let mut report = RestoreReport {
        snapshot_id: plan.snapshot_id.clone(),
        files_restored: 0,
        directories_created: 0,
        symlinks_restored: 0,
        bytes_restored: 0,
        skipped: 0,
        overwritten: 0,
        renamed: 0,
        conflicts: plan.conflicts,
    };

    if opts.dry_run {
        return Ok(report);
    }

    progress.phase("restore", plan.items.len() as u64);
    let mut done = 0u64;
    let mut bytes_done = 0u64;

    for item in &plan.items {
        if progress.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // Refuse to write through a symlinked ancestor: a malicious manifest
        // could otherwise redirect writes outside the destination.
        ensure_safe_destination(&item.dest_path)?;

        match item.kind {
            EntryKind::Directory => {
                if !item.dest_path.exists() {
                    tokio::fs::create_dir_all(&item.dest_path).await?;
                    report.directories_created += 1;
                }
            }
            EntryKind::Symlink => {
                restore_symlink(item, opts, &mut report)?;
            }
            EntryKind::File => {
                let hash = item.hash.as_deref().ok_or_else(|| {
                    Error::InvalidRepository(format!("file {} has no content hash", item.rel_path))
                })?;
                let final_dest = match handle_conflict(&item.dest_path, opts, &mut report)? {
                    Some(p) => p,
                    None => {
                        done += 1;
                        progress.progress(done, plan.items.len() as u64, bytes_done, plan.total_bytes);
                        continue;
                    }
                };
                if let Some(parent) = final_dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                // Download to a temp file in the same directory, then rename.
                let tmp = temp_sibling(&final_dest);
                repo.get_object_to_file(hash, &tmp).await?;
                if opts.preserve_metadata {
                    apply_metadata(&tmp, item).await;
                }
                tokio::fs::rename(&tmp, &final_dest).await?;
                report.files_restored += 1;
                report.bytes_restored += item.size;
                bytes_done += item.size;
            }
        }
        done += 1;
        progress.progress(done, plan.items.len() as u64, bytes_done, plan.total_bytes);
    }

    Ok(report)
}

// -- helpers ---------------------------------------------------------------

fn restore_base(root: &crate::model::RootState, opts: &RestoreOptions) -> PathBuf {
    match (&opts.destination, &opts.root_id) {
        // Whole snapshot to an alternate destination: one folder per root.
        (Some(dest), None) => dest.join(&root.name),
        // Specific root to a destination, or to its original location.
        (Some(dest), Some(_)) => dest.clone(),
        (None, _) => PathBuf::from(&root.local_path),
    }
}

fn resolve_dest(
    base: &Path,
    _root: &crate::model::RootState,
    entry: &FileEntry,
    opts: &RestoreOptions,
    path_filter: &[String],
) -> Option<PathBuf> {
    // Special case: restoring a single file to an explicit file path.
    if path_filter.len() == 1 && entry.kind == EntryKind::File {
        if let Some(dest) = &opts.destination {
            let looks_like_dir = dest.is_dir() || dest.to_string_lossy().ends_with('/');
            if !looks_like_dir && entry.path == path_filter[0] {
                return Some(dest.clone());
            }
        }
    }
    safe_join(base, &entry.path)
}

fn path_matches(path: &str, filters: &[String]) -> bool {
    filters
        .iter()
        .any(|f| path == f || path.starts_with(&format!("{f}/")))
}

/// Guard against symlinked ancestors inside the destination tree.
fn ensure_safe_destination(dest: &Path) -> Result<()> {
    let mut cur = PathBuf::new();
    for comp in dest.components() {
        cur.push(comp);
        if let Ok(meta) = std::fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() && cur != dest {
                return Err(Error::PathTraversal(format!(
                    "refusing to write through symlinked directory {}",
                    cur.display()
                )));
            }
        }
    }
    Ok(())
}

fn handle_conflict(dest: &Path, opts: &RestoreOptions, report: &mut RestoreReport) -> Result<Option<PathBuf>> {
    if !dest.exists() && !is_symlink(dest) {
        return Ok(Some(dest.to_path_buf()));
    }
    match opts.overwrite {
        OverwritePolicy::Refuse => Err(Error::RefuseOverwrite(dest.to_path_buf())),
        OverwritePolicy::Overwrite => {
            report.overwritten += 1;
            Ok(Some(dest.to_path_buf()))
        }
        OverwritePolicy::Skip => {
            report.skipped += 1;
            Ok(None)
        }
        OverwritePolicy::Rename => {
            report.renamed += 1;
            Ok(Some(unique_sibling(dest)))
        }
    }
}

fn restore_symlink(item: &RestoreItem, opts: &RestoreOptions, report: &mut RestoreReport) -> Result<()> {
    let Some(target) = &item.link_target else {
        return Ok(());
    };
    let dest = match handle_conflict(&item.dest_path, opts, report)? {
        Some(p) => p,
        None => return Ok(()),
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &dest)?;
    }
    #[cfg(not(unix))]
    {
        // On platforms without symlink support, record the link as a text file
        // so information is not silently lost.
        std::fs::write(&dest, target)?;
    }
    report.symlinks_restored += 1;
    Ok(())
}

fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn temp_sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let tmp = format!(".{name}.persist-tmp-{}", std::process::id());
    dest.with_file_name(tmp)
}

fn unique_sibling(dest: &Path) -> PathBuf {
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = dest.extension().map(|e| e.to_string_lossy().to_string());
    let parent = dest.parent().unwrap_or(Path::new("."));
    for i in 1..10_000 {
        let candidate = match &ext {
            Some(e) => parent.join(format!("{stem} (restored {i}).{e}")),
            None => parent.join(format!("{stem} (restored {i})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} (restored)"))
}

async fn apply_metadata(path: &Path, item: &RestoreItem) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if item.mode != 0 {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(item.mode));
        }
    }
    if item.mtime > 0 {
        let t = filetime::FileTime::from_unix_time(item.mtime, 0);
        let _ = filetime::set_file_mtime(path, t);
    }
}

/// Group a plan's items by destination root, for reporting.
pub fn items_by_root(plan: &RestorePlan) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for item in &plan.items {
        if item.kind == EntryKind::File {
            *m.entry(item.root_name.clone()).or_insert(0) += 1;
        }
    }
    m
}
