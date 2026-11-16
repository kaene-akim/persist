//! Implementation of every CLI command.

use crate::cli::{ConfigAction, RestoreArgs, Shell};
use crate::output as out;
use crate::progress::{CancelFlag, CliProgress};
use persist_core::config::BackendKind;
use persist_core::engine::{BackupOptions, OverwritePolicy, RestoreOptions};
use persist_core::error::{Error, Result};
use persist_core::model::{ChangeKind, RootConfig, Settings};
use persist_core::session::{slugify, Session};
use persist_core::util::human_size;
use std::path::PathBuf;

/// Shared state for command execution.
pub struct Ctx {
    pub session: Session,
    pub json: bool,
    pub yes: bool,
    pub quiet: bool,
    pub cancel: CancelFlag,
}

impl Ctx {
    fn progress(&self) -> CliProgress {
        CliProgress::new(self.cancel.clone(), out::is_tty() && !self.json)
    }

    /// Ask for confirmation unless `--yes` was passed.
    fn confirm(&self, prompt: &str) -> Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if !out::is_tty() {
            return Err(Error::Message(format!(
                "{prompt}: refusing to continue without --yes in a non-interactive session"
            )));
        }
        dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()
            .map_err(|e| Error::Message(format!("confirmation failed: {e}")))
    }
}

// -- login / account -------------------------------------------------------

pub async fn login(ctx: &mut Ctx) -> Result<()> {
    let info = ctx.session.connect_google().await?;
    if ctx.json {
        out::json(&info);
    } else {
        out::success(&format!("Connected as {} ({})", info.name, info.email));
        out::kv("token storage", &ctx.session.auth_opt().map(|a| a.store_description()).unwrap_or_default());
    }
    Ok(())
}

pub fn logout(ctx: &mut Ctx) -> Result<()> {
    ctx.session.disconnect_google()?;
    if !ctx.json {
        out::success("Disconnected. You can reconnect with `persist login`.");
    }
    Ok(())
}

pub async fn account(ctx: &Ctx) -> Result<()> {
    let info = ctx.session.account().await?;
    if ctx.json {
        out::json(&info);
    } else {
        out::heading("Google account");
        out::kv("name", &info.name);
        out::kv("email", &info.email);
        out::kv("id", &info.sub);
        if let Some(a) = ctx.session.auth_opt() {
            out::kv("token storage", &a.store_description());
        }
    }
    Ok(())
}

// -- repository lifecycle --------------------------------------------------

pub async fn init(
    ctx: &mut Ctx,
    name: String,
    local: Option<PathBuf>,
    drive_folder: Option<String>,
) -> Result<()> {
    if let Some(path) = &local {
        ctx.session.config.backend = BackendKind::Local;
        ctx.session.config.local_path = Some(path.clone());
    }
    if let Some(folder) = drive_folder {
        ctx.session.config.drive_folder = folder;
    }
    ctx.session.save_config()?;
    ctx.session.reset_storage().await;

    if ctx.session.repository_exists().await.unwrap_or(false) {
        return Err(Error::Config(
            "a Persist repository already exists at this location".into(),
        ));
    }
    let meta = ctx.session.init_repository(&name).await?;
    if ctx.json {
        out::json(&meta);
    } else {
        out::success(&format!("Initialised repository {}", meta.repo_id));
        out::kv("name", &meta.name);
        out::kv("backend", &meta.backend);
        out::note("Add a directory with: persist add <path> --remote <destination>");
    }
    Ok(())
}

pub async fn clone(
    ctx: &mut Ctx,
    repository_id: Option<String>,
    destination: Option<PathBuf>,
) -> Result<()> {
    let meta = ctx
        .session
        .clone_repository(repository_id.as_deref(), destination)
        .await?;
    if ctx.json {
        out::json(&meta);
    } else {
        out::success(&format!("Adopted repository {} ({})", meta.repo_id, meta.name));
        out::note("Run `persist log` to see history, then `persist pull` to restore.");
    }
    Ok(())
}

// -- directories -----------------------------------------------------------

pub fn add(
    ctx: &Ctx,
    path: PathBuf,
    remote: Option<String>,
    name: Option<String>,
    exclude: Vec<String>,
) -> Result<()> {
    let canonical = std::fs::canonicalize(&path)
        .map_err(|_| Error::DirectoryMissing(path.clone()))?;
    if !canonical.is_dir() {
        return Err(Error::Config(format!("{} is not a directory", canonical.display())));
    }
    let dir_name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "directory".to_string());
    let name = name.unwrap_or(dir_name);
    let remote_path = remote.unwrap_or_else(|| slugify(&name));
    let root = RootConfig {
        root_id: String::new(),
        name: name.clone(),
        local_path: canonical,
        remote_path: remote_path.clone(),
        exclusions: exclude,
        enabled: true,
        created_at: chrono::Utc::now(),
    };
    let root = ctx.session.add_root(root)?;
    if ctx.json {
        out::json(&root);
    } else {
        out::success(&format!("Added {name}"));
        out::kv("local path", &root.local_path.to_string_lossy());
        out::kv("remote", &root.remote_path);
    }
    Ok(())
}

pub fn remove(ctx: &Ctx, spec: String) -> Result<()> {
    let root = resolve_root(&ctx.session, &spec)?;
    ctx.session.remove_root(&root.root_id)?;
    if ctx.json {
        out::json(&serde_json::json!({ "removed": root.root_id }));
    } else {
        out::success(&format!("Removed {} from management", root.name));
        out::note("Its cloud history and snapshots are untouched.");
    }
    Ok(())
}

pub fn list(ctx: &Ctx) -> Result<()> {
    let roots = ctx.session.roots()?;
    if ctx.json {
        out::json(&serde_json::json!({
            "repository": ctx.session.config.active_repo,
            "backend": ctx.session.config.backend.as_str(),
            "directories": roots,
        }));
        return Ok(());
    }
    out::heading("Managed directories");
    if roots.is_empty() {
        out::note("No directories are managed yet. Use `persist add <path>`.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = roots
        .iter()
        .map(|r| {
            vec![
                r.name.clone(),
                r.local_path.to_string_lossy().to_string(),
                r.remote_path.clone(),
                if r.enabled { "yes".into() } else { "no".into() },
            ]
        })
        .collect();
    out::table(&["NAME", "LOCAL PATH", "REMOTE", "ENABLED"], &rows);
    out::kv("backend", ctx.session.config.backend.as_str());
    if let Some(id) = &ctx.session.config.active_repo {
        out::kv("repository", id);
    }
    Ok(())
}

// -- backup ----------------------------------------------------------------

pub async fn push(
    ctx: &mut Ctx,
    path: Option<String>,
    dry_run: bool,
    message: Option<String>,
) -> Result<()> {
    let only_root = match path {
        Some(spec) => Some(resolve_root(&ctx.session, &spec)?.root_id),
        None => None,
    };
    let plan = ctx.session.plan_backup(only_root.as_deref()).await?;

    if ctx.json && dry_run {
        out::json(&plan);
        return Ok(());
    }
    if !ctx.json && !ctx.quiet {
        print_plan(&plan);
    }
    if dry_run {
        if !ctx.json {
            out::note("Dry run: nothing was uploaded.");
        }
        return Ok(());
    }

    if plan.uploads.is_empty() {
        if ctx.json {
            out::json(&serde_json::json!({ "up_to_date": true, "changes": plan.change_set }));
        } else {
            out::success("Everything is already backed up; no changes to upload.");
        }
        return Ok(());
    }

    let prompt = format!(
        "Upload {} new object(s) ({}) and create a snapshot?",
        plan.uploads.len(),
        human_size(plan.upload_bytes)
    );
    if !ctx.confirm(&prompt)? {
        return Err(Error::Cancelled);
    }

    let progress = ctx.progress();
    let opts = BackupOptions {
        dry_run: false,
        only_root,
        message,
    };
    let result = ctx.session.run_backup(&opts, &progress).await?;
    if ctx.json {
        out::json(&result);
    } else if !ctx.quiet {
        out::success(&format!("Snapshot {} created", result.snapshot_id));
        out::kv("files", &result.files_backed_up.to_string());
        out::kv("logical size", &human_size(result.logical_size));
        out::kv("new objects", &result.new_objects.to_string());
        out::kv("uploaded", &human_size(result.new_bytes));
        out::kv("added / modified / deleted", &format!("{} / {} / {}", result.added, result.modified, result.deleted));
        out::kv("duration", &format!("{:.1}s", result.duration_secs));
    }
    Ok(())
}

fn print_plan(plan: &persist_core::model::BackupPlan) {
    out::heading(if plan.is_initial {
        "Initial backup preview"
    } else {
        "Backup preview"
    });
    out::kv("files", &plan.total_files.to_string());
    out::kv("logical size", &human_size(plan.total_logical_size));
    out::kv("new / modified / deleted", &format!("{} / {} / {}", plan.change_set.added, plan.change_set.modified, plan.change_set.deleted));
    out::kv("unchanged", &plan.change_set.unchanged.to_string());
    out::kv("objects to upload", &plan.uploads.len().to_string());
    out::kv("estimated upload", &human_size(plan.upload_bytes));
    out::kv("excluded files", &format!("{} ({})", plan.excluded_files, human_size(plan.excluded_bytes)));

    let changed: Vec<&persist_core::model::FileChange> = plan
        .change_set
        .changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Unchanged)
        .take(50)
        .collect();
    if !changed.is_empty() {
        println!();
        let rows: Vec<Vec<String>> = changed
            .iter()
            .map(|c| {
                vec![
                    match c.kind {
                        ChangeKind::Added => "add".into(),
                        ChangeKind::Modified => "mod".into(),
                        ChangeKind::Deleted => "del".into(),
                        ChangeKind::Unchanged => "".into(),
                    },
                    c.root_name.clone(),
                    c.path.clone(),
                    human_size(c.new.as_ref().or(c.old.as_ref()).map(|e| e.size).unwrap_or(0)),
                ]
            })
            .collect();
        out::table(&["", "DIRECTORY", "PATH", "SIZE"], &rows);
        if plan.change_set.changes.iter().filter(|c| c.kind != ChangeKind::Unchanged).count() > 50 {
            out::note("… (more changes not shown)");
        }
    }
}

pub async fn status(ctx: &Ctx) -> Result<()> {
    let roots = ctx.session.roots()?;
    let exists = ctx.session.repository_exists().await.unwrap_or(false);
    if !exists {
        if ctx.json {
            out::json(&serde_json::json!({ "repository": null, "initialised": false }));
        } else {
            out::warn("No repository found. Run `persist init` to create one.");
        }
        return Ok(());
    }
    let head = ctx.session.head_snapshot().await?;
    let snapshots = ctx.session.snapshots(1000).await?;
    let info = ctx.session.storage_info().await?;

    if ctx.json {
        out::json(&serde_json::json!({
            "repository": ctx.session.config.active_repo,
            "backend": info.backend,
            "head": head.as_ref().map(|s| &s.snapshot_id),
            "snapshots": snapshots.len(),
            "usage": info.usage,
            "quota": info.quota,
            "directories": roots.len(),
        }));
        return Ok(());
    }

    out::heading("Repository");
    out::kv("backend", &info.backend);
    out::kv("repository", ctx.session.config.active_repo.as_deref().unwrap_or("-"));
    out::kv("snapshots", &snapshots.len().to_string());
    match &head {
        Some(s) => {
            out::kv("latest snapshot", &s.snapshot_id);
            out::kv("latest backup", &out::timestamp(s.created_at));
            out::kv("protected files", &s.stats.file_count.to_string());
        }
        None => out::kv("latest snapshot", "none"),
    }
    out::kv("logical storage", &human_size(info.usage.logical_bytes));
    out::kv("physical storage", &human_size(info.usage.physical_bytes));
    out::kv("objects", &info.usage.object_count.to_string());
    if let Some(q) = &info.quota {
        if q.limit > 0 {
            out::kv("drive remaining", &human_size(q.remaining()));
        }
    }
    if !roots.is_empty() {
        out::heading("Directories");
        let rows: Vec<Vec<String>> = roots
            .iter()
            .map(|r| vec![r.name.clone(), r.local_path.to_string_lossy().to_string(), r.remote_path.clone()])
            .collect();
        out::table(&["NAME", "LOCAL PATH", "REMOTE"], &rows);
    }
    Ok(())
}

// -- restore ---------------------------------------------------------------

pub async fn pull(ctx: &mut Ctx, args: RestoreArgs, snapshot: Option<String>) -> Result<()> {
    let opts = restore_options(&ctx.session, &args, snapshot)?;
    run_restore(ctx, opts).await
}

fn restore_options(
    session: &Session,
    args: &RestoreArgs,
    snapshot: Option<String>,
) -> Result<RestoreOptions> {
    let overwrite = if args.overwrite {
        OverwritePolicy::Overwrite
    } else if args.skip {
        OverwritePolicy::Skip
    } else if args.rename {
        OverwritePolicy::Rename
    } else {
        OverwritePolicy::Refuse
    };
    // `--root` accepts a root id, a name or a local path.
    let root_id = match &args.root {
        Some(spec) => Some(resolve_root(session, spec)?.root_id),
        None => None,
    };
    Ok(RestoreOptions {
        snapshot_id: snapshot,
        root_id,
        paths: args.paths.clone(),
        destination: args.destination.clone(),
        overwrite,
        dry_run: args.dry_run,
        preserve_metadata: true,
    })
}

async fn run_restore(ctx: &mut Ctx, opts: RestoreOptions) -> Result<()> {
    let (snapshot, plan) = ctx.session.plan_restore(&opts).await?;

    // A dry run only ever reports what would happen.
    if opts.dry_run {
        if ctx.json {
            out::json(&plan);
        } else {
            print_restore_plan(&snapshot.snapshot_id, &plan);
            out::note("Dry run: nothing was written.");
        }
        return Ok(());
    }

    // Never overwrite silently: refuse before touching the filesystem.
    if plan.conflicts > 0 && opts.overwrite == OverwritePolicy::Refuse {
        return Err(Error::RefuseOverwrite(PathBuf::from(format!(
            "{} file(s) already exist. Re-run with --overwrite, --skip or --rename",
            plan.conflicts
        ))));
    }

    if !ctx.json {
        print_restore_plan(&snapshot.snapshot_id, &plan);
    }
    if !ctx.confirm("Proceed with the restore?")? {
        return Err(Error::Cancelled);
    }

    let progress = ctx.progress();
    let report = ctx.session.restore(&opts, &progress).await?;
    if ctx.json {
        out::json(&report);
    } else {
        out::success(&format!("Restored {} file(s)", report.files_restored));
        out::kv("bytes", &human_size(report.bytes_restored));
        out::kv("skipped", &report.skipped.to_string());
        out::kv("overwritten", &report.overwritten.to_string());
        out::kv("renamed", &report.renamed.to_string());
    }
    Ok(())
}

fn print_restore_plan(snapshot_id: &str, plan: &persist_core::engine::RestorePlan) {
    out::heading(&format!("Restore preview (snapshot {snapshot_id})"));
    out::kv("files", &plan.files.to_string());
    out::kv("directories", &plan.directories.to_string());
    out::kv("symlinks", &plan.symlinks.to_string());
    out::kv("total size", &human_size(plan.total_bytes));
    out::kv("conflicts", &plan.conflicts.to_string());
}

// -- history ---------------------------------------------------------------

pub async fn log(ctx: &Ctx, limit: u32) -> Result<()> {
    let snapshots = ctx.session.snapshots(limit).await?;
    if ctx.json {
        out::json(&snapshots);
        return Ok(());
    }
    out::heading("Snapshot history");
    if snapshots.is_empty() {
        out::note("No snapshots yet. Run `persist push` to create one.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = snapshots
        .iter()
        .map(|s| {
            vec![
                s.snapshot_id.clone(),
                out::timestamp(s.created_at),
                s.parent.clone().unwrap_or_else(|| "-".into()),
                s.stats.file_count.to_string(),
                human_size(s.stats.logical_size),
                format!("+{} ~{} -{}", s.stats.added, s.stats.modified, s.stats.deleted),
            ]
        })
        .collect();
    out::table(
        &["SNAPSHOT", "CREATED", "PARENT", "FILES", "SIZE", "CHANGES"],
        &rows,
    );
    Ok(())
}

pub async fn diff(
    ctx: &Ctx,
    snapshot_a: Option<String>,
    snapshot_b: Option<String>,
    text: bool,
) -> Result<()> {
    let change_set = match (snapshot_a, snapshot_b) {
        (None, None) => ctx.session.diff_local(None).await?,
        (Some(a), None) => ctx.session.diff_snapshots(&a, &latest_or_error(ctx).await?).await?,
        (Some(a), Some(b)) => ctx.session.diff_snapshots(&a, &b).await?,
        (None, Some(_)) => {
            return Err(Error::Config(
                "diff needs either no arguments, one snapshot, or two snapshots".into(),
            ))
        }
    };

    if ctx.json {
        out::json(&change_set);
        return Ok(());
    }

    let changed: Vec<&persist_core::model::FileChange> = change_set
        .changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Unchanged)
        .collect();
    out::heading("Changes");
    if changed.is_empty() {
        out::note("No differences.");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = changed
        .iter()
        .map(|c| {
            vec![
                match c.kind {
                    ChangeKind::Added => "add".into(),
                    ChangeKind::Modified => "mod".into(),
                    ChangeKind::Deleted => "del".into(),
                    ChangeKind::Unchanged => "".into(),
                },
                c.root_name.clone(),
                c.path.clone(),
            ]
        })
        .collect();
    out::table(&["", "DIRECTORY", "PATH"], &rows);
    out::kv("added / modified / deleted", &format!("{} / {} / {}", change_set.added, change_set.modified, change_set.deleted));

    if text {
        print_text_diffs(ctx, &change_set).await?;
    }
    Ok(())
}

async fn latest_or_error(ctx: &Ctx) -> Result<String> {
    ctx.session
        .head_snapshot()
        .await?
        .map(|s| s.snapshot_id)
        .ok_or_else(|| Error::SnapshotNotFound("no snapshots exist".into()))
}

async fn print_text_diffs(ctx: &Ctx, change_set: &persist_core::model::ChangeSet) -> Result<()> {
    let repo = ctx.session.repository().await?;
    let mut printed = 0;
    for change in change_set
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Modified)
    {
        if printed >= 10 {
            out::note("… (more text diffs not shown)");
            break;
        }
        let (Some(old), Some(new)) = (change.old.as_ref(), change.new.as_ref()) else {
            continue;
        };
        let (Some(old_hash), Some(new_hash)) = (old.hash.as_deref(), new.hash.as_deref()) else {
            continue;
        };
        // Only diff plausibly textual, small files.
        if old.size > 2 * 1024 * 1024 || new.size > 2 * 1024 * 1024 {
            continue;
        }
        let old_bytes = repo.get_object(old_hash).await?;
        let new_bytes = repo.get_object(new_hash).await?;
        let (Ok(old_text), Ok(new_text)) = (
            String::from_utf8(old_bytes),
            String::from_utf8(new_bytes),
        ) else {
            out::note(&format!("{}: binary file, skipping diff", change.path));
            continue;
        };
        println!("\n{} {}", console::style("diff").bold(), change.path);
        for line in unified_diff(&old_text, &new_text) {
            if line.starts_with('+') {
                println!("{}", console::style(&line).green());
            } else if line.starts_with('-') {
                println!("{}", console::style(&line).red());
            } else {
                println!("{line}");
            }
        }
        printed += 1;
    }
    Ok(())
}

/// A small line-oriented LCS diff. Suitable for the small text files shown by
/// `persist diff --text`.
pub fn unified_diff(old: &str, new: &str) -> Vec<String> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let n = a.len();
    let m = b.len();
    // Guard against pathological inputs.
    if n.saturating_mul(m) > 4_000_000 {
        return vec![format!("(file too large to diff: {n} -> {m} lines)")];
    }
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(format!(" {}", a[i]));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(format!("-{}", a[i]));
            i += 1;
        } else {
            out.push(format!("+{}", b[j]));
            j += 1;
        }
    }
    while i < n {
        out.push(format!("-{}", a[i]));
        i += 1;
    }
    while j < m {
        out.push(format!("+{}", b[j]));
        j += 1;
    }
    out
}

// -- storage / verify ------------------------------------------------------

pub async fn storage(ctx: &Ctx) -> Result<()> {
    let info = ctx.session.storage_info().await?;
    if ctx.json {
        out::json(&info);
        return Ok(());
    }
    out::heading("Storage");
    out::kv("backend", &info.backend);
    if let Some(q) = &info.quota {
        if q.limit > 0 {
            out::kv("drive quota", &human_size(q.limit));
            out::kv("drive used", &human_size(q.usage));
            out::kv("drive remaining", &human_size(q.remaining()));
            out::kv("drive trash", &human_size(q.usage_in_drive_trash));
        } else {
            out::kv("drive used", &human_size(q.usage));
        }
    } else {
        out::note("The provider did not report a quota.");
    }
    println!();
    out::kv("persist objects", &info.usage.object_count.to_string());
    out::kv("persist physical", &human_size(info.usage.physical_bytes));
    out::kv("persist logical (latest)", &human_size(info.usage.logical_bytes));
    out::kv("logical across history", &human_size(info.usage.total_logical_bytes));
    out::kv("snapshots", &info.usage.snapshot_count.to_string());
    out::note("Logical storage is the size of the files themselves; physical storage is what is actually stored. Deduplication keeps physical below the logical total.");
    Ok(())
}

pub async fn verify(ctx: &Ctx, deep: bool) -> Result<()> {
    let progress = ctx.progress();
    let report = ctx.session.verify(deep, &progress).await?;
    if ctx.json {
        out::json(&report);
    } else {
        out::heading("Verification");
        out::kv("snapshots checked", &report.snapshots_checked.to_string());
        out::kv("objects referenced", &report.objects_referenced.to_string());
        out::kv("objects checked", &report.objects_checked.to_string());
        out::kv("missing", &report.missing_objects.len().to_string());
        out::kv("corrupt", &report.corrupt_objects.len().to_string());
        out::kv("orphans", &report.orphan_objects.len().to_string());
        if report.ok {
            out::success("Repository is healthy.");
        } else {
            out::error("Repository has integrity problems.");
            for m in report.missing_objects.iter().take(10) {
                out::warn(&format!("missing object {m}"));
            }
            for c in report.corrupt_objects.iter().take(10) {
                out::warn(&format!("corrupt object {}", c.hash));
            }
        }
    }
    if !report.ok {
        return Err(Error::Message("repository verification failed".into()));
    }
    Ok(())
}

// -- config ----------------------------------------------------------------

pub fn config(ctx: &mut Ctx, action: Option<ConfigAction>) -> Result<()> {
    match action {
        None => {
            if ctx.json {
                out::json(&serde_json::json!({
                    "config": ctx.session.config,
                    "settings": ctx.session.settings(),
                }));
            } else {
                out::heading("Configuration");
                out::kv("backend", ctx.session.config.backend.as_str());
                out::kv("repository name", &ctx.session.config.repo_name);
                out::kv("active repository", ctx.session.config.active_repo.as_deref().unwrap_or("-"));
                out::kv("drive folder", &ctx.session.config.drive_folder);
                if let Some(p) = &ctx.session.config.local_path {
                    out::kv("local path", &p.to_string_lossy());
                }
                out::kv("config file", &ctx.session.paths.config_file().to_string_lossy());
                let s = ctx.session.settings();
                out::heading("Settings");
                out::kv("verify uploads", &s.verify_uploads.to_string());
                out::kv("upload concurrency", &s.upload_concurrency.to_string());
                out::kv("follow symlinks", &s.follow_symlinks.to_string());
                out::kv("store symlinks", &s.store_symlinks.to_string());
                out::kv("max file size", &s.max_file_size.to_string());
                out::kv("upload chunk size", &s.upload_chunk_size.to_string());
                out::kv("default exclusions", &s.default_exclusions.join(", "));
            }
        }
        Some(ConfigAction::Get { key }) => {
            let value = get_config_value(ctx, &key)?;
            if ctx.json {
                out::json(&serde_json::json!({ "key": key, "value": value }));
            } else {
                println!("{value}");
            }
        }
        Some(ConfigAction::Set { key, value }) => {
            set_config_value(ctx, &key, &value)?;
            ctx.session.save_config()?;
            if ctx.json {
                out::json(&serde_json::json!({ "key": key, "value": value }));
            } else {
                out::success(&format!("Set {key} = {value}"));
            }
        }
    }
    Ok(())
}

fn get_config_value(ctx: &Ctx, key: &str) -> Result<String> {
    let s = ctx.session.settings();
    Ok(match key {
        "backend" => ctx.session.config.backend.as_str().to_string(),
        "repo_name" | "repository_name" => ctx.session.config.repo_name.clone(),
        "drive_folder" => ctx.session.config.drive_folder.clone(),
        "local_path" => ctx
            .session
            .config
            .local_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        "verify_uploads" => s.verify_uploads.to_string(),
        "upload_concurrency" => s.upload_concurrency.to_string(),
        "follow_symlinks" => s.follow_symlinks.to_string(),
        "store_symlinks" => s.store_symlinks.to_string(),
        "max_file_size" => s.max_file_size.to_string(),
        "upload_chunk_size" => s.upload_chunk_size.to_string(),
        "default_exclusions" => s.default_exclusions.join(","),
        other => return Err(Error::Config(format!("unknown configuration key: {other}"))),
    })
}

fn set_config_value(ctx: &mut Ctx, key: &str, value: &str) -> Result<()> {
    let mut s: Settings = ctx.session.settings();
    let parse_bool = |v: &str| matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on");
    match key {
        "backend" => {
            ctx.session.config.backend = match value {
                "local" => BackendKind::Local,
                "google-drive" | "drive" | "gdrive" => BackendKind::GoogleDrive,
                other => return Err(Error::Config(format!("unknown backend: {other}"))),
            };
        }
        "repo_name" | "repository_name" => ctx.session.config.repo_name = value.to_string(),
        "drive_folder" => ctx.session.config.drive_folder = value.to_string(),
        "local_path" => ctx.session.config.local_path = Some(PathBuf::from(value)),
        "verify_uploads" => s.verify_uploads = parse_bool(value),
        "upload_concurrency" => {
            s.upload_concurrency = value
                .parse()
                .map_err(|_| Error::Config("upload_concurrency must be a number".into()))?
        }
        "follow_symlinks" => s.follow_symlinks = parse_bool(value),
        "store_symlinks" => s.store_symlinks = parse_bool(value),
        "max_file_size" => {
            s.max_file_size = value
                .parse()
                .map_err(|_| Error::Config("max_file_size must be a number of bytes".into()))?
        }
        "upload_chunk_size" => {
            s.upload_chunk_size = value
                .parse()
                .map_err(|_| Error::Config("upload_chunk_size must be a number of bytes".into()))?
        }
        "default_exclusions" => {
            s.default_exclusions = value
                .split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        }
        other => return Err(Error::Config(format!("unknown configuration key: {other}"))),
    }
    ctx.session.set_settings(&s)?;
    Ok(())
}

// -- completions -----------------------------------------------------------

pub fn completions(shell: Shell) {
    use clap::CommandFactory;
    let mut cmd = crate::cli::Cli::command();
    let name = cmd.get_name().to_string();
    let sh: clap_complete::Shell = shell.into();
    clap_complete::generate(sh, &mut cmd, name, &mut std::io::stdout());
}

// -- helpers ---------------------------------------------------------------

/// Resolve a directory specification to a managed root by id, name or path.
pub fn resolve_root(session: &Session, spec: &str) -> Result<RootConfig> {
    let roots = session.roots()?;
    if let Some(r) = roots.iter().find(|r| {
        r.root_id == spec || r.name == spec || r.local_path.to_string_lossy() == spec
    }) {
        return Ok(r.clone());
    }
    if let Ok(canon) = std::fs::canonicalize(spec) {
        if let Some(r) = roots.iter().find(|r| {
            std::fs::canonicalize(&r.local_path)
                .map(|c| c == canon)
                .unwrap_or(false)
        }) {
            return Ok(r.clone());
        }
    }
    Err(Error::RootNotFound(spec.to_string()))
}
