//! Tests for the "recover on a brand new machine" story and for the local
//! filesystem backend acting as a full repository.

use persist_core::engine::{self, NoopProgress, RestoreOptions};
use persist_core::model::{RootConfig, Settings};
use persist_core::repo::Repository;
use persist_core::storage::local::LocalStorage;
use persist_core::storage::memory::MemoryStorage;
use persist_core::storage::StorageRef;
use std::sync::Arc;

async fn make_project(path: &std::path::Path) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("a.txt"), b"alpha").unwrap();
    std::fs::write(path.join("b.txt"), b"beta").unwrap();
}

#[tokio::test]
async fn recover_from_cloud_on_a_new_machine() {
    // "Machine 1": create a repository and back up.
    let workdir = tempfile::tempdir().unwrap();
    let project = workdir.path().join("project");
    make_project(&project).await;
    let cloud = Arc::new(MemoryStorage::new());

    let repo = Arc::new(
        Repository::init(cloud.clone() as StorageRef, "photos", None)
            .await
            .unwrap(),
    );
    let repo_id = repo.repo_id().to_string();
    let root = RootConfig {
        root_id: "r1".into(),
        name: "Project".into(),
        local_path: project.clone(),
        remote_path: "project".into(),
        exclusions: vec![],
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    let settings = Settings::default();
    let plan = engine::plan_backup(&repo, &[root], &settings, None).await.unwrap();
    let result = engine::execute_backup(&repo, &plan, &settings, &NoopProgress)
        .await
        .unwrap();
    drop(repo);

    // "Machine 2": only the cloud storage (the same in-memory backend stands in
    // for Google Drive) is available. No local database exists.
    let repo2 = Arc::new(Repository::open(cloud.clone() as StorageRef).await.unwrap());
    assert_eq!(repo2.repo_id(), repo_id);
    // The index can be rebuilt purely from snapshot manifests.
    let n = repo2.rebuild_index().await.unwrap();
    assert_eq!(n, 2);
    let ids = repo2.list_snapshot_ids().await.unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], result.snapshot_id);
    assert_eq!(repo2.head().await.unwrap().unwrap(), result.snapshot_id);

    // Restore to a fresh destination.
    let dest = workdir.path().join("recovered");
    let opts = RestoreOptions {
        destination: Some(dest.clone()),
        ..Default::default()
    };
    let (_s, plan) = engine::plan_restore(&repo2, &opts).await.unwrap();
    assert_eq!(plan.files, 2);
    engine::restore(&repo2, &plan, &opts, &NoopProgress).await.unwrap();
    assert_eq!(std::fs::read(dest.join("Project/a.txt")).unwrap(), b"alpha");
    assert_eq!(std::fs::read(dest.join("Project/b.txt")).unwrap(), b"beta");
}

#[tokio::test]
async fn local_backend_roundtrip() {
    let workdir = tempfile::tempdir().unwrap();
    let project = workdir.path().join("project");
    make_project(&project).await;
    let repo_dir = workdir.path().join("repo");

    let storage = Arc::new(LocalStorage::new(&repo_dir));
    let repo = Arc::new(
        Repository::init(storage.clone() as StorageRef, "local", None)
            .await
            .unwrap(),
    );
    let root = RootConfig {
        root_id: "r1".into(),
        name: "Project".into(),
        local_path: project.clone(),
        remote_path: "project".into(),
        exclusions: vec![],
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    let settings = Settings::default();
    let plan = engine::plan_backup(&repo, &[root.clone()], &settings, None)
        .await
        .unwrap();
    engine::execute_backup(&repo, &plan, &settings, &NoopProgress)
        .await
        .unwrap();

    // Incremental: modify one file.
    std::fs::write(project.join("a.txt"), b"ALPHA").unwrap();
    let plan = engine::plan_backup(&repo, &[root], &settings, None)
        .await
        .unwrap();
    assert_eq!(plan.uploads.len(), 1);
    engine::execute_backup(&repo, &plan, &settings, &NoopProgress)
        .await
        .unwrap();

    // Reopen from disk and verify.
    let repo2 = Arc::new(Repository::open(storage as StorageRef).await.unwrap());
    let report = engine::verify_repository(&repo2, true, &NoopProgress)
        .await
        .unwrap();
    assert!(report.ok, "verification failed: {report:?}");
    assert_eq!(repo2.list_snapshot_ids().await.unwrap().len(), 2);
}
