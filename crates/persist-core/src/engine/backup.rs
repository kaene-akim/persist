//! Backup planning and execution.
//!
//! The two phases are intentionally separate so that interfaces can show a
//! preview (including a `--dry-run` that uploads nothing) before committing.
//!
//! Guarantees:
//! * A snapshot is only written *after* every object it references has been
//!   uploaded (and, when verification is enabled, re-downloaded and hashed).
//! * A failure never produces a snapshot that looks successful.
//! * Re-running after an interruption re-uses already uploaded objects.

use super::Progress;
use crate::error::{Error, Result};
use crate::exclude::ExcludeRules;
use crate::model::{
    BackupPlan, BackupResult, ChangeKind, EntryKind, PlannedRoot, PlannedUpload, RootConfig,
    RootState, Settings, Snapshot, SnapshotStats, SNAPSHOT_FORMAT_VERSION,
};
use crate::repo::Repository;
use crate::scanner::{self, ScanOptions};
use crate::util;
use chrono::Utc;
use futures::stream::{self, StreamExt};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// Options for a backup run.
#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    /// Analyse only; never upload or commit.
    pub dry_run: bool,
    /// Restrict the run to a single managed root id.
    pub only_root: Option<String>,
    /// Optional snapshot message.
    pub message: Option<String>,
}

/// Scan the configured roots and work out what would need to be uploaded.
pub async fn plan_backup(
    repo: &Repository,
    roots: &[RootConfig],
    settings: &Settings,
    only_root: Option<&str>,
) -> Result<BackupPlan> {
    let head_id = repo.head().await?;
    let head = match &head_id {
        Some(id) => Some(repo.load_snapshot(id).await?),
        None => None,
    };
    let head_roots: BTreeMap<String, RootState> = head
        .as_ref()
        .map(|s| {
            s.roots
                .iter()
                .map(|r| (r.root_id.clone(), r.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut new_roots: Vec<RootState> = Vec::new();
    let mut upload_candidates: HashMap<String, (u64, PathBuf)> = HashMap::new();
    let mut excluded_files = 0u64;
    let mut excluded_bytes = 0u64;

    for root in roots.iter().filter(|r| r.enabled) {
        if let Some(only) = only_root {
            if root.root_id != only {
                // Carry forward the previous state so the snapshot stays a
                // complete picture of everything Persist protects.
                if let Some(prev) = head_roots.get(&root.root_id) {
                    new_roots.push(prev.clone());
                }
                continue;
            }
        }

        if !root.local_path.exists() {
            return Err(Error::DirectoryMissing(root.local_path.clone()));
        }

        let patterns = combined_exclusions(settings, root);
        let rules = ExcludeRules::compile(&patterns)?;
        let scan_opts = ScanOptions {
            exclusions: rules,
            follow_symlinks: settings.follow_symlinks,
            store_symlinks: settings.store_symlinks,
            max_file_size: settings.max_file_size,
            concurrency: settings.upload_concurrency.max(2),
        };
        let result = scanner::scan(&root.local_path, &scan_opts).await?;
        excluded_files += result.excluded_files;
        excluded_bytes += result.excluded_bytes;

        for entry in &result.entries {
            if let Some(hash) = &entry.hash {
                upload_candidates.entry(hash.clone()).or_insert_with(|| {
                    (entry.size, root.local_path.join(entry.path.split('/').collect::<PathBuf>()))
                });
            }
        }

        new_roots.push(RootState {
            root_id: root.root_id.clone(),
            name: root.name.clone(),
            local_path: root.local_path.to_string_lossy().to_string(),
            remote_path: root.remote_path.clone(),
            entries: result.entries,
            exclusions: patterns,
        });
    }

    new_roots.sort_by(|a, b| a.remote_path.cmp(&b.remote_path).then(a.name.cmp(&b.name)));

    // Anything already present in the head snapshot is definitely stored.
    let head_hashes: HashSet<String> = head
        .as_ref()
        .map(|s| s.referenced_hashes().into_iter().collect())
        .unwrap_or_default();

    let mut uploads = Vec::new();
    for (hash, (size, path)) in upload_candidates {
        if head_hashes.contains(&hash) {
            continue;
        }
        if repo.object_exists(&hash).await? {
            continue;
        }
        uploads.push(PlannedUpload {
            hash,
            size,
            local_path: path,
        });
    }
    uploads.sort_by(|a, b| a.hash.cmp(&b.hash));

    let old_roots: Vec<RootState> = head
        .as_ref()
        .map(|s| s.roots.clone())
        .unwrap_or_default();
    let change_set = super::diff::diff_roots(&old_roots, &new_roots);

    let total_files = new_roots
        .iter()
        .flat_map(|r| r.entries.iter())
        .filter(|e| e.kind == EntryKind::File)
        .count() as u64;
    let total_logical_size = new_roots
        .iter()
        .flat_map(|r| r.entries.iter())
        .map(|e| e.size)
        .sum();
    let upload_bytes: u64 = uploads.iter().map(|u| u.size).sum();

    Ok(BackupPlan {
        roots: new_roots
            .into_iter()
            .map(|state| PlannedRoot {
                root_id: state.root_id.clone(),
                name: state.name.clone(),
                local_path: state.local_path.clone(),
                remote_path: state.remote_path.clone(),
                state,
            })
            .collect(),
        uploads,
        change_set,
        total_files,
        total_logical_size,
        upload_bytes,
        excluded_files,
        excluded_bytes,
        is_initial: head.is_none(),
        head_snapshot: head_id,
    })
}

/// Upload everything in `plan` and commit a new snapshot.
pub async fn execute_backup(
    repo: &Repository,
    plan: &BackupPlan,
    settings: &Settings,
    progress: &dyn Progress,
) -> Result<BackupResult> {
    let started = Instant::now();
    let total_uploads = plan.uploads.len() as u64;
    let total_bytes = plan.upload_bytes;

    progress.phase("upload", total_uploads);
    progress.message(&format!(
        "Uploading {} new object(s), {}",
        total_uploads,
        util::human_size(total_bytes)
    ));

    let concurrency = settings.upload_concurrency.max(1);
    let verify = settings.verify_uploads;

    // Upload objects with bounded concurrency. Each task returns the number of
    // bytes uploaded (0 when the object already existed).
    let results: Vec<Result<u64>> = stream::iter(plan.uploads.iter().cloned().map(|upload| {
        async move {
            if progress.is_cancelled() {
                return Err(Error::Cancelled);
            }
            // Guard against a file changing between scanning and uploading.
            let meta = tokio::fs::metadata(&upload.local_path)
                .await
                .map_err(|_| Error::Message(format!(
                    "{} disappeared during backup; please retry",
                    upload.local_path.display()
                )))?;
            if meta.len() != upload.size {
                return Err(Error::Message(format!(
                    "{} changed size during backup; please retry",
                    upload.local_path.display()
                )));
            }
            let uploaded = repo
                .put_object_from_file(&upload.hash, upload.size, &upload.local_path)
                .await?;
            if !uploaded {
                return Ok(0);
            }
            if verify {
                // Re-download and hash to prove the stored bytes are correct.
                let data = repo.storage().get(&crate::repo::layout::object_path(&upload.hash)).await?;
                let actual = crate::hashing::hash_bytes(&data);
                if actual != upload.hash {
                    // Remove the bad object so it cannot be mistaken for valid.
                    let _ = repo
                        .storage()
                        .delete(&crate::repo::layout::object_path(&upload.hash))
                        .await;
                    return Err(Error::CorruptObject {
                        hash: upload.hash.clone(),
                        expected: upload.hash.clone(),
                        actual,
                    });
                }
            }
            Ok(upload.size)
        }
    }))
    .buffered(concurrency)
    .collect()
    .await;

    let mut done = 0u64;
    let mut bytes_done = 0u64;
    let mut new_objects = 0u64;
    let mut new_bytes = 0u64;
    for r in results {
        match r {
            Ok(b) => {
                if b > 0 {
                    new_objects += 1;
                    new_bytes += b;
                }
                bytes_done += b;
                done += 1;
                progress.progress(done, total_uploads, bytes_done, total_bytes);
            }
            Err(e) => return Err(e),
        }
    }

    // Commit the index before the manifest so a crash here only causes harmless
    // re-uploads, never a manifest referencing a missing object.
    repo.flush_index().await?;

    let snapshot_id = util::new_snapshot_id();
    let deleted: Vec<String> = plan
        .change_set
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Deleted)
        .map(|c| format!("{}:{}", c.root_id, c.path))
        .collect();

    let mut stats = SnapshotStats {
        file_count: 0,
        dir_count: 0,
        symlink_count: 0,
        logical_size: 0,
        new_objects,
        new_bytes,
        added: plan.change_set.added,
        modified: plan.change_set.modified,
        deleted: plan.change_set.deleted,
    };
    for root in &plan.roots {
        for e in &root.state.entries {
            match e.kind {
                EntryKind::File => {
                    stats.file_count += 1;
                    stats.logical_size += e.size;
                }
                EntryKind::Directory => stats.dir_count += 1,
                EntryKind::Symlink => stats.symlink_count += 1,
            }
        }
    }

    let snapshot = Snapshot {
        format_version: SNAPSHOT_FORMAT_VERSION,
        snapshot_id: snapshot_id.clone(),
        parent: plan.head_snapshot.clone(),
        created_at: Utc::now(),
        message: plan_message(plan, settings),
        roots: plan.roots.iter().map(|r| r.state.clone()).collect(),
        deleted,
        stats: stats.clone(),
    };

    progress.phase("commit", 1);
    repo.save_snapshot(&snapshot).await?;
    repo.set_head(Some(snapshot_id.clone())).await?;
    progress.progress(1, 1, total_bytes, total_bytes);

    Ok(BackupResult {
        snapshot_id,
        parent: plan.head_snapshot.clone(),
        created_at: snapshot.created_at,
        files_backed_up: stats.file_count,
        logical_size: stats.logical_size,
        new_objects,
        new_bytes,
        added: plan.change_set.added,
        modified: plan.change_set.modified,
        deleted: plan.change_set.deleted,
        unchanged: plan.change_set.unchanged,
        duration_secs: started.elapsed().as_secs_f64(),
    })
}

fn plan_message(plan: &BackupPlan, _settings: &Settings) -> Option<String> {
    // The caller can override this via `BackupOptions::message`; a default is
    // derived from whether this was an initial or incremental run.
    if plan.is_initial {
        Some("Initial backup".to_string())
    } else {
        None
    }
}

/// Combine global defaults, per-root exclusions.
pub fn combined_exclusions(settings: &Settings, root: &RootConfig) -> Vec<String> {
    let mut v = settings.default_exclusions.clone();
    v.extend(root.exclusions.iter().cloned());
    v
}
