//! End-to-end tests for the `persist` binary.
//!
//! These run the real executable as a child process, using a temporary
//! `PERSIST_HOME` and a local repository backend so that no Google credentials
//! or network access are required. They verify exit codes, JSON output,
//! non-interactive behaviour and the full local workflow.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Path to the compiled binary, provided by Cargo.
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_persist")
}

/// A disposable test environment: a home directory plus a working tree.
struct Env {
    dir: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        Self { dir, home, repo }
    }

    fn work(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Run `persist` with the given args, returning the raw output.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(bin())
            .args(args)
            .env("PERSIST_HOME", &self.home)
            .env_remove("PERSIST_CLIENT_ID")
            .env_remove("PERSIST_CLIENT_SECRET")
            // Ensure a non-tty stdout so confirmations refuse rather than block.
            .output()
            .expect("failed to run persist")
    }

    /// Run and require success, returning stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "command {args:?} failed (code {:?}):\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Run and require success, parsing stdout as JSON.
    fn json(&self, args: &[&str]) -> Value {
        let stdout = self.ok(args);
        serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("command {args:?} did not emit JSON ({e}):\n{stdout}"))
    }

    /// Initialise a local repository and add a project directory with files.
    fn setup_project(&self) -> PathBuf {
        let project = self.work().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("main.py"), b"print('hello')\n").unwrap();
        std::fs::write(project.join("config.json"), b"{\"a\":1}\n").unwrap();
        std::fs::write(project.join("README.md"), b"# readme\n").unwrap();

        self.ok(&[
            "init",
            "--local",
            self.repo.to_str().unwrap(),
            "--json",
        ]);
        self.ok(&[
            "add",
            project.to_str().unwrap(),
            "--remote",
            "project",
            "--json",
        ]);
        project
    }
}

#[test]
fn help_lists_every_documented_command() {
    let env = Env::new();
    let out = env.run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in [
        "init", "add", "remove", "list", "clone", "push", "status", "pull", "restore", "log",
        "diff", "account", "storage", "verify", "config", "completions",
    ] {
        assert!(text.contains(cmd), "help output is missing `{cmd}`:\n{text}");
    }
}

#[test]
fn invalid_subcommand_exits_with_clap_code() {
    let env = Env::new();
    let out = env.run(&["definitely-not-a-command"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn init_creates_repository_and_reports_json() {
    let env = Env::new();
    let meta = env.json(&["init", "--local", env.repo.to_str().unwrap(), "--json"]);
    assert_eq!(meta["backend"], "local");
    assert!(meta["repo_id"].as_str().unwrap().len() >= 16);
    assert!(env.repo.join("repository.json").exists());
    assert!(env.repo.join("refs/head.json").exists());
}

#[test]
fn init_twice_is_an_error() {
    let env = Env::new();
    env.ok(&["init", "--local", env.repo.to_str().unwrap()]);
    let out = env.run(&["init", "--local", env.repo.to_str().unwrap()]);
    assert!(!out.status.success());
    // Config error class.
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn add_missing_directory_reports_exit_code_five() {
    let env = Env::new();
    env.ok(&["init", "--local", env.repo.to_str().unwrap()]);
    let out = env.run(&["add", "/no/such/directory/xyz"]);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn add_and_list_roundtrip() {
    let env = Env::new();
    env.setup_project();
    let list = env.json(&["list", "--json"]);
    let dirs = list["directories"].as_array().unwrap();
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0]["remote_path"], "project");
    assert_eq!(list["backend"], "local");
}

#[test]
fn remove_directory_keeps_history() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    env.ok(&["remove", "project", "--json"]);
    let list = env.json(&["list", "--json"]);
    assert!(list["directories"].as_array().unwrap().is_empty());
    // The snapshot still exists in the repository.
    let log = env.json(&["log", "--json"]);
    assert_eq!(log.as_array().unwrap().len(), 1);
}

#[test]
fn dry_run_uploads_nothing_and_emits_plan() {
    let env = Env::new();
    env.setup_project();
    let plan = env.json(&["push", "--dry-run", "--json"]);
    assert_eq!(plan["uploads"].as_array().unwrap().len(), 3);
    assert_eq!(plan["total_files"], 3);
    // No snapshot must have been created.
    let log = env.json(&["log", "--json"]);
    assert!(log.as_array().unwrap().is_empty());
}

#[test]
fn push_creates_snapshot_and_incremental_upload() {
    let env = Env::new();
    let project = env.setup_project();

    let first = env.json(&["push", "--yes", "--json"]);
    assert_eq!(first["files_backed_up"], 3);
    assert_eq!(first["new_objects"], 3);

    // Modify one file, add one, delete one.
    std::fs::write(project.join("main.py"), b"print('changed')\n").unwrap();
    std::fs::write(project.join("utils.py"), b"def f(): pass\n").unwrap();
    std::fs::remove_file(project.join("README.md")).unwrap();

    let second = env.json(&["push", "--yes", "--json"]);
    // Only the two genuinely new contents are uploaded.
    assert_eq!(second["new_objects"], 2);
    assert_eq!(second["modified"], 1);
    assert_eq!(second["added"], 1);
    assert_eq!(second["deleted"], 1);
    assert_eq!(second["parent"], first["snapshot_id"]);

    let log = env.json(&["log", "--json"]);
    assert_eq!(log.as_array().unwrap().len(), 2);
}

#[test]
fn push_without_yes_refuses_in_non_interactive_mode() {
    let env = Env::new();
    env.setup_project();
    let out = env.run(&["push"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--yes"),
        "expected a hint about --yes, got: {stderr}"
    );
    // Still no snapshot.
    let log = env.json(&["log", "--json"]);
    assert!(log.as_array().unwrap().is_empty());
}

#[test]
fn second_push_with_no_changes_reports_up_to_date() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    let result = env.json(&["push", "--yes", "--json"]);
    assert_eq!(result["up_to_date"], true);
}

#[test]
fn status_reports_repository_information() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    let status = env.json(&["status", "--json"]);
    assert_eq!(status["backend"], "local");
    assert_eq!(status["snapshots"], 1);
    assert_eq!(status["directories"], 1);
    assert!(status["usage"]["object_count"].as_u64().unwrap() >= 3);
}

#[test]
fn status_without_repository_is_not_an_error() {
    let env = Env::new();
    let status = env.json(&["status", "--json"]);
    assert_eq!(status["initialised"], false);
}

#[test]
fn storage_reports_logical_and_physical() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    let info = env.json(&["storage", "--json"]);
    assert_eq!(info["backend"], "local");
    let usage = &info["usage"];
    assert!(usage["physical_bytes"].as_u64().unwrap() > 0);
    assert!(usage["logical_bytes"].as_u64().unwrap() > 0);
}

#[test]
fn verify_succeeds_and_reports_json() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    let report = env.json(&["verify", "--json"]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["snapshots_checked"], 1);
    assert!(report["missing_objects"].as_array().unwrap().is_empty());
}

#[test]
fn verify_detects_corruption_and_exits_nonzero() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);

    // Corrupt an object behind Persist's back.
    let objects_dir = env.repo.join("objects");
    let mut corrupted = false;
    for shard in std::fs::read_dir(&objects_dir).unwrap() {
        let shard = shard.unwrap().path();
        if shard.is_dir() {
            for obj in std::fs::read_dir(&shard).unwrap() {
                let obj = obj.unwrap().path();
                std::fs::write(&obj, b"corrupted contents").unwrap();
                corrupted = true;
                break;
            }
        }
        if corrupted {
            break;
        }
    }
    assert!(corrupted, "no objects found to corrupt");

    let out = env.run(&["verify", "--deep", "--json"]);
    assert!(!out.status.success());
    let report: Value = serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["corrupt_objects"].as_array().unwrap().len(), 1);
}

#[test]
fn json_errors_are_structured_on_stderr() {
    let env = Env::new();
    env.ok(&["init", "--local", env.repo.to_str().unwrap()]);
    let out = env.run(&["add", "/no/such/dir", "--json"]);
    assert_eq!(out.status.code(), Some(5));
    let payload: Value = serde_json::from_str(&String::from_utf8_lossy(&out.stderr)).unwrap();
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"]["code"], "directory_missing");
}

#[test]
fn pull_restores_entire_snapshot_to_a_destination() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);
    let dest = env.work().join("recovered");
    let report = env.json(&[
        "pull",
        "--destination",
        dest.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    assert_eq!(report["files_restored"], 3);
    // Whole-snapshot restore creates one folder per root.
    assert!(dest.join("project/main.py").exists());
    assert!(dest.join("project/config.json").exists());
    assert!(dest.join("project/README.md").exists());
}

#[test]
fn pull_refuses_to_overwrite_existing_files() {
    let env = Env::new();
    let project = env.setup_project();
    env.ok(&["push", "--yes"]);
    // Restore back into the original location where files exist.
    let out = env.run(&["pull", "--yes"]);
    assert_eq!(out.status.code(), Some(7));
    // The local file is untouched.
    assert_eq!(std::fs::read(project.join("main.py")).unwrap(), b"print('hello')\n");
}

#[test]
fn pull_overwrite_replaces_local_changes() {
    let env = Env::new();
    let project = env.setup_project();
    env.ok(&["push", "--yes"]);
    std::fs::write(project.join("main.py"), b"locally edited\n").unwrap();
    env.ok(&["pull", "--yes", "--overwrite"]);
    assert_eq!(std::fs::read(project.join("main.py")).unwrap(), b"print('hello')\n");
}

#[test]
fn restore_single_file_from_older_snapshot() {
    let env = Env::new();
    let project = env.setup_project();
    let first = env.json(&["push", "--yes", "--json"]);
    let first_id = first["snapshot_id"].as_str().unwrap().to_string();

    std::fs::write(project.join("main.py"), b"print('v2')\n").unwrap();
    env.ok(&["push", "--yes"]);

    // Restore the older revision to an explicit file path.
    let dest = env.work().join("old-main.py");
    let report = env.json(&[
        "restore",
        &first_id,
        "--path",
        "main.py",
        "--destination",
        dest.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    assert_eq!(report["files_restored"], 1);
    assert_eq!(std::fs::read(&dest).unwrap(), b"print('hello')\n");
    // The current working copy is left untouched.
    assert_eq!(
        std::fs::read(project.join("main.py")).unwrap(),
        b"print('v2')\n"
    );
}

#[test]
fn restore_single_file_into_a_destination_directory() {
    let env = Env::new();
    env.setup_project();
    env.ok(&["push", "--yes"]);

    // A trailing separator marks the destination as a directory, so the file
    // keeps its relative name inside it.
    let dest = env.work().join("out");
    let dest_arg = format!("{}/", dest.display());
    let report = env.json(&[
        "pull",
        "--root",
        "project",
        "--path",
        "main.py",
        "--destination",
        &dest_arg,
        "--yes",
        "--json",
    ]);
    assert_eq!(report["files_restored"], 1);
    assert_eq!(
        std::fs::read(dest.join("main.py")).unwrap(),
        b"print('hello')\n"
    );
}

#[test]
fn log_limit_is_respected() {
    let env = Env::new();
    let project = env.setup_project();
    for i in 0..3 {
        std::fs::write(project.join("main.py"), format!("print({i})\n")).unwrap();
        env.ok(&["push", "--yes"]);
    }
    let log = env.json(&["log", "--limit", "2", "--json"]);
    assert_eq!(log.as_array().unwrap().len(), 2);
}

#[test]
fn diff_local_and_between_snapshots() {
    let env = Env::new();
    let project = env.setup_project();
    let first = env.json(&["push", "--yes", "--json"]);
    let first_id = first["snapshot_id"].as_str().unwrap().to_string();

    std::fs::write(project.join("main.py"), b"print('changed')\n").unwrap();
    std::fs::write(project.join("added.txt"), b"new\n").unwrap();
    let second = env.json(&["push", "--yes", "--json"]);
    let second_id = second["snapshot_id"].as_str().unwrap().to_string();

    let local = env.json(&["diff", "--json"]);
    assert_eq!(local["modified"], 0);
    assert_eq!(local["added"], 0);

    let between = env.json(&["diff", &first_id, &second_id, "--json"]);
    assert_eq!(between["modified"], 1);
    assert_eq!(between["added"], 1);
}

#[test]
fn config_get_and_set() {
    let env = Env::new();
    env.ok(&["init", "--local", env.repo.to_str().unwrap()]);
    let got = env.json(&["config", "get", "upload_concurrency", "--json"]);
    assert_eq!(got["value"], "4");

    env.ok(&["config", "set", "upload_concurrency", "7", "--json"]);
    let got = env.json(&["config", "get", "upload_concurrency", "--json"]);
    assert_eq!(got["value"], "7");

    // Unknown keys are rejected.
    let out = env.run(&["config", "get", "nonsense"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn completions_are_generated_for_all_shells() {
    let env = Env::new();
    for shell in ["bash", "zsh", "fish"] {
        let out = env.run(&["completions", shell]);
        assert!(out.status.success(), "completions {shell} failed");
        assert!(
            !out.stdout.is_empty(),
            "completions {shell} produced no output"
        );
    }
}

#[test]
fn account_without_login_reports_not_connected() {
    let env = Env::new();
    let out = env.run(&["account", "--json"]);
    // No OAuth client is configured in the test environment.
    assert!(!out.status.success());
    assert_ne!(out.status.code(), Some(0));
}

#[test]
fn clone_rebuilds_history_from_the_cloud() {
    // Machine 1 backs up, then "machine 2" (a fresh home) clones and restores.
    let env = Env::new();
    let project = env.setup_project();
    env.ok(&["push", "--yes"]);

    // Point a brand new home at the same local repository and clone it.
    let home2 = env.work().join("home2");
    std::fs::create_dir_all(&home2).unwrap();
    let dest = env.work().join("restored");
    let out = Command::new(bin())
        .args([
            "init",
            "--local",
            env.repo.to_str().unwrap(),
            "--json",
        ])
        .env("PERSIST_HOME", &home2)
        .output()
        .unwrap();
    // init fails because the repository already exists; that is expected.
    assert!(!out.status.success());

    // Configure the backend then clone explicitly.
    let out = Command::new(bin())
        .args(["config", "set", "backend", "local"])
        .env("PERSIST_HOME", &home2)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new(bin())
        .args([
            "config",
            "set",
            "local_path",
            env.repo.to_str().unwrap(),
        ])
        .env("PERSIST_HOME", &home2)
        .output()
        .unwrap();
    assert!(out.status.success());

    let out = Command::new(bin())
        .args(["clone", "--json"])
        .env("PERSIST_HOME", &home2)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clone failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = Command::new(bin())
        .args([
            "pull",
            "--destination",
            dest.to_str().unwrap(),
            "--yes",
            "--json",
        ])
        .env("PERSIST_HOME", &home2)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "pull failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dest.join("project/main.py")).unwrap(),
        b"print('hello')\n"
    );
    // The original project is irrelevant to the recovery.
    let _ = project;
}

#[test]
fn quiet_flag_suppresses_human_summary() {
    let env = Env::new();
    env.setup_project();
    let stdout = env.ok(&["push", "--yes", "--quiet"]);
    assert!(
        !stdout.contains("Snapshot history"),
        "quiet output was not minimal: {stdout}"
    );
}

fn _assert_path_type(_p: &Path) {}
