//! Repository integrity verification.
//!
//! `verify` answers the question "can I actually recover from this repository?".
//! It checks that every snapshot parses, that every object a snapshot references
//! is present, and (in deep mode) that every object's contents still hash to its
//! address.

use super::Progress;
use crate::error::Result;
use crate::hashing;
use crate::repo::{layout, Repository};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A single corrupt object found during verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorruptRecord {
    pub hash: String,
    pub expected: String,
    pub actual: String,
}

/// The result of a verification run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifyReport {
    pub snapshots_checked: u64,
    pub objects_referenced: u64,
    pub objects_checked: u64,
    pub missing_objects: Vec<String>,
    pub corrupt_objects: Vec<CorruptRecord>,
    pub snapshot_issues: Vec<String>,
    /// Objects present in the index but not referenced by any snapshot.
    pub orphan_objects: Vec<String>,
    /// Objects referenced by a snapshot but absent from the index.
    pub missing_from_index: Vec<String>,
    pub ok: bool,
}

/// Verify the repository.
///
/// When `deep` is true every object is downloaded and re-hashed; otherwise only
/// existence is checked, which is much cheaper for large repositories.
pub async fn verify_repository(
    repo: &Repository,
    deep: bool,
    progress: &dyn Progress,
) -> Result<VerifyReport> {
    let mut report = VerifyReport {
        ok: true,
        ..Default::default()
    };

    progress.phase("snapshots", 0);
    let ids = repo.list_snapshot_ids().await?;
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for id in &ids {
        match repo.load_snapshot(id).await {
            Ok(snap) => {
                report.snapshots_checked += 1;
                for h in snap.referenced_hashes() {
                    referenced.insert(h);
                }
                // The parent link should resolve (except for the root snapshot).
                if let Some(parent) = &snap.parent {
                    if !ids.contains(parent) {
                        report.snapshot_issues.push(format!(
                            "snapshot {id} references missing parent {parent}"
                        ));
                    }
                }
            }
            Err(e) => {
                report.snapshot_issues.push(format!("snapshot {id}: {e}"));
            }
        }
    }
    report.objects_referenced = referenced.len() as u64;

    let indexed = repo.all_objects().await?;
    for h in &referenced {
        if !indexed.contains_key(h) {
            report.missing_from_index.push(h.clone());
        }
    }
    for h in indexed.keys() {
        if !referenced.contains(h) {
            report.orphan_objects.push(h.clone());
        }
    }

    // Check objects. Prefer checking referenced objects; also verify index
    // entries so orphans are not silently corrupt.
    let mut to_check: BTreeSet<String> = referenced.clone();
    to_check.extend(indexed.keys().cloned());
    let list: Vec<String> = to_check.into_iter().collect();

    progress.phase("objects", list.len() as u64);
    let concurrency = 8;
    let results: Vec<(String, Result<Option<CorruptRecord>>)> =
        stream::iter(list.iter().cloned().map(|hash| {
            let repo = repo;
            async move {
                let path = layout::object_path(&hash);
                match repo.storage().exists(&path).await {
                    Ok(false) => {
                        let rec = CorruptRecord {
                            hash: hash.clone(),
                            expected: hash.clone(),
                            actual: "<missing>".into(),
                        };
                        (hash, Ok(Some(rec)))
                    }
                    Ok(true) => {
                        if deep {
                            match repo.storage().get(&path).await {
                                Ok(data) => {
                                    let actual = hashing::hash_bytes(&data);
                                    if actual != hash {
                                        let rec = CorruptRecord {
                                            hash: hash.clone(),
                                            expected: hash.clone(),
                                            actual,
                                        };
                                        (hash, Ok(Some(rec)))
                                    } else {
                                        (hash, Ok(None))
                                    }
                                }
                                Err(e) => (hash, Err(e)),
                            }
                        } else {
                            (hash, Ok(None))
                        }
                    }
                    Err(e) => (hash, Err(e)),
                }
            }
        }))
        .buffered(concurrency)
        .collect()
        .await;

    let mut done = 0u64;
    for (hash, res) in results {
        done += 1;
        report.objects_checked += 1;
        match res {
            Ok(None) => {}
            Ok(Some(rec)) => {
                if rec.actual == "<missing>" {
                    report.missing_objects.push(hash);
                } else {
                    report.corrupt_objects.push(rec);
                }
                report.ok = false;
            }
            Err(e) => {
                report.snapshot_issues.push(format!("object {hash}: {e}"));
                report.ok = false;
            }
        }
        progress.progress(done, list.len() as u64, 0, 0);
    }

    if !report.snapshot_issues.is_empty()
        || !report.missing_objects.is_empty()
        || !report.corrupt_objects.is_empty()
    {
        report.ok = false;
    }
    Ok(report)
}
