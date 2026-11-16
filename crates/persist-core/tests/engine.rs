//! End-to-end tests for the backup engine against the in-memory storage
//! backend. These exercise the real engine, repository format and restore code
//! without touching the network.

use persist_core::engine::{self, NoopProgress, OverwritePolicy, RestoreOptions};
use persist_core::model::{EntryKind, RootConfig, Settings};
use persist_core::repo::Repository;
use persist_core::storage::memory::MemoryStorage;
use persist_core::storage::{Storage, StorageRef};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Fixture {
    _dir: tempfile::TempDir,
    storage: Arc<MemoryStorage>,
    repo: Arc<Repository>,
    root_path: PathBuf,
    root: RootConfig,
    settings: Settings,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root_path = dir.path().join("project");
    std::fs::create_dir_all(&root_path).unwrap();
    std::fs::write(root_path.join("main.py"), b"print('hello')\n").unwrap();
    std::fs::write(root_path.join("config.json"), b"{\"a\":1}\n").unwrap();
    std::fs::write(root_path.join("README.md"), b"# readme\n").unwrap();

    let storage = Arc::new(MemoryStorage::new());
    let repo = Arc::new(
        Repository::init(storage.clone() as StorageRef, "test", None)
            .await
            .unwrap(),
    );
    let root = RootConfig {
        root_id: "root-1".into(),
        name: "Project".into(),
        local_path: root_path.clone(),
        remote_path: "project".into(),
        exclusions: vec![],
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    Fixture {
        _dir: dir,
        storage,
        repo,
        root_path,
        root,
        settings: Settings::default(),
    }
}

async fn backup(fx: &Fixture) -> persist_core::model::BackupResult {
    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    engine::execute_backup(&fx.repo, &plan, &fx.settings, &NoopProgress)
        .await
        .unwrap()
}

#[tokio::test]
async fn initial_backup_stores_everything() {
    let fx = fixture().await;
    let result = backup(&fx).await;
    assert_eq!(result.files_backed_up, 3);
    assert_eq!(result.new_objects, 3);
    assert!(fx.repo.head().await.unwrap().is_some());
    // Three distinct objects plus repository.json, head, snapshot and index.
    let usage = fx.repo.usage().await.unwrap();
    assert_eq!(usage.object_count, 3);
    assert_eq!(usage.snapshot_count, 1);
    assert_eq!(usage.logical_bytes, result.logical_size);
}

#[tokio::test]
async fn incremental_backup_uploads_only_changes() {
    let fx = fixture().await;
    backup(&fx).await;

    // Modify one file and add one.
    std::fs::write(fx.root_path.join("main.py"), b"print('changed')\n").unwrap();
    std::fs::write(fx.root_path.join("utils.py"), b"def f(): pass\n").unwrap();

    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    // Only main.py and utils.py need uploading; config/README are unchanged.
    assert_eq!(plan.uploads.len(), 2);
    assert_eq!(plan.change_set.added, 1);
    assert_eq!(plan.change_set.modified, 1);
    assert_eq!(plan.change_set.unchanged, 2);

    let result = engine::execute_backup(&fx.repo, &plan, &fx.settings, &NoopProgress)
        .await
        .unwrap();
    assert_eq!(result.new_objects, 2);
    assert_eq!(result.files_backed_up, 4);

    let usage = fx.repo.usage().await.unwrap();
    assert_eq!(usage.object_count, 5); // 3 original + 2 new
    assert_eq!(usage.snapshot_count, 2);
    // Two snapshots share objects, so total logical > physical.
    assert!(usage.total_logical_bytes > usage.physical_bytes);
}

#[tokio::test]
async fn duplicate_content_is_stored_once() {
    let fx = fixture().await;
    std::fs::write(fx.root_path.join("copy1.txt"), b"identical").unwrap();
    std::fs::write(fx.root_path.join("copy2.txt"), b"identical").unwrap();
    let result = backup(&fx).await;
    // 3 originals + 1 shared duplicate = 4 objects, not 5.
    assert_eq!(result.new_objects, 4);
}

#[tokio::test]
async fn cross_directory_deduplication() {
    let fx = fixture().await;
    let other_path = fx.root_path.parent().unwrap().join("other");
    std::fs::create_dir_all(&other_path).unwrap();
    std::fs::write(other_path.join("main_copy.py"), b"print('hello')\n").unwrap();
    let other = RootConfig {
        root_id: "root-2".into(),
        name: "Other".into(),
        local_path: other_path,
        remote_path: "other".into(),
        exclusions: vec![],
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    let roots = vec![fx.root.clone(), other];
    let plan = engine::plan_backup(&fx.repo, &roots, &fx.settings, None)
        .await
        .unwrap();
    engine::execute_backup(&fx.repo, &plan, &fx.settings, &NoopProgress)
        .await
        .unwrap();
    // main.py content is identical in both roots -> stored once, so the four
    // files collapse to three unique objects.
    let usage = fx.repo.usage().await.unwrap();
    assert_eq!(usage.object_count, 3);
}

#[tokio::test]
async fn deletion_is_recorded_and_object_preserved() {
    let fx = fixture().await;
    backup(&fx).await;
    let head1 = fx.repo.head().await.unwrap().unwrap();

    std::fs::remove_file(fx.root_path.join("README.md")).unwrap();
    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    assert_eq!(plan.change_set.deleted, 1);
    assert!(plan.uploads.is_empty(), "deleting must not upload anything");
    engine::execute_backup(&fx.repo, &plan, &fx.settings, &NoopProgress)
        .await
        .unwrap();

    // The old snapshot still references the deleted file's object.
    let usage = fx.repo.usage().await.unwrap();
    assert_eq!(usage.object_count, 3);

    // Restore the deleted file from the first snapshot.
    let dest = fx.root_path.parent().unwrap().join("restored");
    let opts = RestoreOptions {
        snapshot_id: Some(head1),
        root_id: Some("root-1".into()),
        paths: vec!["README.md".into()],
        destination: Some(dest.join("README.md")),
        overwrite: OverwritePolicy::Refuse,
        dry_run: false,
        preserve_metadata: false,
    };
    let (_snap, plan) = engine::plan_restore(&fx.repo, &opts).await.unwrap();
    assert_eq!(plan.files, 1);
    let report = engine::restore(&fx.repo, &plan, &opts, &NoopProgress)
        .await
        .unwrap();
    assert_eq!(report.files_restored, 1);
    assert_eq!(
        std::fs::read(dest.join("README.md")).unwrap(),
        b"# readme\n"
    );
}

#[tokio::test]
async fn rename_does_not_upload_content() {
    let fx = fixture().await;
    backup(&fx).await;
    std::fs::rename(
        fx.root_path.join("main.py"),
        fx.root_path.join("app.py"),
    )
    .unwrap();
    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    assert!(plan.uploads.is_empty(), "renamed content already exists");
    assert_eq!(plan.change_set.added, 1);
    assert_eq!(plan.change_set.deleted, 1);
}

#[tokio::test]
async fn full_restore_to_alternate_destination() {
    let fx = fixture().await;
    let result = backup(&fx).await;
    let dest = fx.root_path.parent().unwrap().join("recovered");
    let opts = RestoreOptions {
        snapshot_id: Some(result.snapshot_id),
        destination: Some(dest.clone()),
        ..Default::default()
    };
    let (_s, plan) = engine::plan_restore(&fx.repo, &opts).await.unwrap();
    assert_eq!(plan.files, 3);
    let report = engine::restore(&fx.repo, &plan, &opts, &NoopProgress)
        .await
        .unwrap();
    assert_eq!(report.files_restored, 3);
    // Whole-snapshot restore to a destination creates one folder per root.
    let project = dest.join("Project");
    assert!(project.join("main.py").exists());
    assert!(project.join("config.json").exists());
    assert!(project.join("README.md").exists());
}

#[tokio::test]
async fn restore_refuses_to_overwrite_by_default() {
    let fx = fixture().await;
    let result = backup(&fx).await;
    // Restore into the original location where files already exist.
    let opts = RestoreOptions {
        snapshot_id: Some(result.snapshot_id),
        ..Default::default()
    };
    let (_s, plan) = engine::plan_restore(&fx.repo, &opts).await.unwrap();
    assert!(plan.conflicts >= 3);
    let err = engine::restore(&fx.repo, &plan, &opts, &NoopProgress).await;
    assert!(matches!(
        err,
        Err(persist_core::Error::RefuseOverwrite(_))
    ));
}

#[tokio::test]
async fn restore_skip_and_overwrite_policies() {
    let fx = fixture().await;
    let result = backup(&fx).await;
    std::fs::write(fx.root_path.join("main.py"), b"locally edited").unwrap();

    let skip = RestoreOptions {
        snapshot_id: Some(result.snapshot_id.clone()),
        overwrite: OverwritePolicy::Skip,
        ..Default::default()
    };
    let (_s, plan) = engine::plan_restore(&fx.repo, &skip).await.unwrap();
    let report = engine::restore(&fx.repo, &plan, &skip, &NoopProgress)
        .await
        .unwrap();
    assert!(report.skipped >= 1);
    assert_eq!(
        std::fs::read(fx.root_path.join("main.py")).unwrap(),
        b"locally edited"
    );

    let overwrite = RestoreOptions {
        snapshot_id: Some(result.snapshot_id),
        overwrite: OverwritePolicy::Overwrite,
        ..Default::default()
    };
    let (_s, plan) = engine::plan_restore(&fx.repo, &overwrite).await.unwrap();
    let report = engine::restore(&fx.repo, &plan, &overwrite, &NoopProgress)
        .await
        .unwrap();
    assert!(report.overwritten >= 1);
    assert_eq!(
        std::fs::read(fx.root_path.join("main.py")).unwrap(),
        b"print('hello')\n"
    );
}

#[tokio::test]
async fn manifest_path_traversal_is_rejected() {
    let fx = fixture().await;
    backup(&fx).await;
    let head = fx.repo.head().await.unwrap().unwrap();
    let mut snap = fx.repo.load_snapshot(&head).await.unwrap();
    snap.roots[0].entries.push(persist_core::model::FileEntry {
        path: "../../etc/passwd".into(),
        kind: EntryKind::File,
        size: 3,
        hash: Some(persist_core::hashing::hash_bytes(b"abc")),
        link_target: None,
        mode: 0o644,
        mtime: 0,
    });
    assert!(snap.validate().is_err());
}

#[tokio::test]
async fn verify_detects_missing_object() {
    let fx = fixture().await;
    backup(&fx).await;
    // Delete one object behind the repository's back.
    let objects = fx.repo.all_objects().await.unwrap();
    let victim = objects.keys().next().unwrap().clone();
    fx.storage
        .delete(&persist_core::repo::layout::object_path(&victim))
        .await
        .unwrap();

    let report = engine::verify_repository(&fx.repo, true, &NoopProgress)
        .await
        .unwrap();
    assert!(!report.ok);
    assert!(report.missing_objects.contains(&victim));
}

#[tokio::test]
async fn verify_detects_corrupt_object() {
    let fx = fixture().await;
    backup(&fx).await;
    let objects = fx.repo.all_objects().await.unwrap();
    let victim = objects.keys().next().unwrap().clone();
    // Overwrite with content that hashes differently.
    fx.storage
        .put(
            &persist_core::repo::layout::object_path(&victim),
            b"corrupted!",
        )
        .await
        .unwrap();
    let report = engine::verify_repository(&fx.repo, true, &NoopProgress)
        .await
        .unwrap();
    assert!(!report.ok);
    assert_eq!(report.corrupt_objects.len(), 1);
}

#[tokio::test]
async fn interrupted_upload_leaves_no_snapshot_and_recovers() {
    let fx = fixture().await;
    // Fail every 4th write: enough to break the run partway through.
    fx.storage.fail_every(4);
    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    let res = engine::execute_backup(&fx.repo, &plan, &fx.settings, &NoopProgress).await;
    assert!(res.is_err());
    // No snapshot must have been committed.
    assert!(fx.repo.head().await.unwrap().is_none());
    assert!(fx.repo.list_snapshot_ids().await.unwrap().is_empty());

    // Disable failures and retry: it must succeed.
    fx.storage.fail_every(0);
    let result = backup(&fx).await;
    assert_eq!(result.files_backed_up, 3);
    assert!(fx.repo.head().await.unwrap().is_some());
}

#[tokio::test]
async fn dry_run_commits_nothing() {
    let fx = fixture().await;
    let plan = engine::plan_backup(&fx.repo, std::slice::from_ref(&fx.root), &fx.settings, None)
        .await
        .unwrap();
    assert_eq!(plan.uploads.len(), 3);
    // The plan is read-only: no snapshot exists.
    assert!(fx.repo.head().await.unwrap().is_none());
    assert!(fx.repo.list_snapshot_ids().await.unwrap().is_empty());
}

#[tokio::test]
async fn snapshot_parent_chain_is_recorded() {
    let fx = fixture().await;
    let first = backup(&fx).await;
    std::fs::write(fx.root_path.join("new.txt"), b"new").unwrap();
    let second = backup(&fx).await;
    assert_eq!(second.parent.as_deref(), Some(first.snapshot_id.as_str()));
    let snap = fx.repo.load_snapshot(&second.snapshot_id).await.unwrap();
    assert_eq!(snap.parent.as_deref(), Some(first.snapshot_id.as_str()));
    assert_eq!(snap.stats.added, 1);
}

#[tokio::test]
async fn snapshot_diff_reports_changes() {
    let fx = fixture().await;
    let first = backup(&fx).await;
    std::fs::write(fx.root_path.join("main.py"), b"changed").unwrap();
    std::fs::write(fx.root_path.join("added.txt"), b"new").unwrap();
    std::fs::remove_file(fx.root_path.join("README.md")).unwrap();
    let second = backup(&fx).await;

    let a = fx.repo.load_snapshot(&first.snapshot_id).await.unwrap();
    let b = fx.repo.load_snapshot(&second.snapshot_id).await.unwrap();
    let cs = engine::diff::diff_snapshots(Some(&a), &b);
    assert_eq!(cs.added, 1);
    assert_eq!(cs.modified, 1);
    assert_eq!(cs.deleted, 1);
    assert_eq!(cs.unchanged, 1);
}

#[tokio::test]
async fn exclusions_are_respected_and_reported() {
    let fx = fixture().await;
    std::fs::create_dir_all(fx.root_path.join("node_modules/pkg")).unwrap();
    std::fs::write(fx.root_path.join("node_modules/pkg/index.js"), b"x").unwrap();
    std::fs::write(fx.root_path.join("debug.log"), b"log line").unwrap();

    let mut root = fx.root.clone();
    root.exclusions = vec!["node_modules/".into(), "*.log".into()];
    let plan = engine::plan_backup(&fx.repo, &[root], &fx.settings, None)
        .await
        .unwrap();
    assert_eq!(plan.excluded_files, 2);
    assert!(plan.excluded_bytes > 0);
    assert_eq!(plan.uploads.len(), 3);
}

fn _assert_path_type(_p: &Path) {}
