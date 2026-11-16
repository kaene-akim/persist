//! Command line interface definition.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "persist",
    version,
    about = "Incremental, versioned backups to Google Drive",
    long_about = "Persist protects selected local directories by storing incremental, \
                  deduplicated, versioned snapshots in Google Drive (or a local \
                  repository). Every snapshot is independently recoverable.",
    propagate_version = true
)]
pub struct Cli {
    /// Output machine readable JSON instead of human readable text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Assume "yes" for confirmations (non-interactive).
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,

    /// Reduce output to a minimum.
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    /// Override the Persist home directory (config + state).
    #[arg(long, global = true, env = "PERSIST_HOME")]
    pub home: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Connect a Google account (runs the OAuth flow).
    Login {
        /// Do not try to open a browser automatically.
        #[arg(long)]
        no_browser: bool,
    },

    /// Forget the connected Google account.
    Logout,

    /// Display the connected Google account.
    Account,

    /// Initialise a new Persist repository.
    Init {
        /// Repository name.
        #[arg(default_value = "Persist")]
        name: String,
        /// Use a local directory as the repository instead of Google Drive.
        #[arg(long, value_name = "PATH")]
        local: Option<PathBuf>,
        /// Name of the folder to create in Google Drive.
        #[arg(long, value_name = "NAME")]
        drive_folder: Option<String>,
    },

    /// Discover and adopt an existing cloud repository on this machine.
    Clone {
        /// Expected repository id (optional; verified when given).
        repository_id: Option<String>,
        /// Where recovered directories should live.
        destination: Option<PathBuf>,
    },

    /// Add a local directory to backup management.
    Add {
        /// Local directory to protect.
        path: PathBuf,
        /// Destination inside the repository.
        #[arg(long, value_name = "DEST")]
        remote: Option<String>,
        /// Descriptive name (defaults to the directory name).
        #[arg(long)]
        name: Option<String>,
        /// Exclusion pattern (repeatable).
        #[arg(long = "exclude", value_name = "GLOB")]
        exclude: Vec<String>,
    },

    /// Remove a directory from management (cloud history is preserved).
    Remove {
        /// Local path or name of the managed directory.
        path: String,
    },

    /// List managed directories and repository information.
    List,

    /// Create a new backup (snapshot).
    Push {
        /// Back up a specific managed directory (path or name).
        path: Option<String>,
        /// Analyse only; upload nothing.
        #[arg(long)]
        dry_run: bool,
        /// Attach a message to the snapshot.
        #[arg(long, short = 'm')]
        message: Option<String>,
    },

    /// Show repository status and backup information.
    Status,

    /// Restore the latest snapshot (or a specific one).
    Pull {
        /// Snapshot id to restore (defaults to the latest).
        #[arg(long)]
        snapshot: Option<String>,
        /// Restore to this directory instead of the original location.
        #[arg(long, value_name = "PATH")]
        destination: Option<PathBuf>,
        /// Restrict the restore to one managed directory.
        #[arg(long, value_name = "ROOT")]
        root: Option<String>,
        /// Restore only these paths (relative, repeatable).
        #[arg(long = "path", value_name = "REL")]
        paths: Vec<String>,
        /// Replace files that already exist.
        #[arg(long, conflicts_with_all = ["skip", "rename"])]
        overwrite: bool,
        /// Leave existing files untouched.
        #[arg(long)]
        skip: bool,
        /// Write alongside existing files with a "(restored)" suffix.
        #[arg(long)]
        rename: bool,
        /// Analyse only; write nothing.
        #[arg(long)]
        dry_run: bool,
    },

    /// Restore a specific snapshot (with safety checks).
    Restore {
        /// Snapshot id to restore.
        snapshot_id: String,
        #[command(flatten)]
        options: RestoreArgs,
    },

    /// Display snapshot history.
    Log {
        /// Maximum number of snapshots to show.
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },

    /// Compare the working tree with the latest snapshot, or two snapshots.
    Diff {
        /// First snapshot id.
        snapshot_a: Option<String>,
        /// Second snapshot id.
        snapshot_b: Option<String>,
        /// Show a text diff for changed text files.
        #[arg(long)]
        text: bool,
    },

    /// Display Google Drive quota and Persist storage usage.
    Storage,

    /// Verify repository integrity and stored object hashes.
    Verify {
        /// Download and re-hash every object (slower but thorough).
        #[arg(long)]
        deep: bool,
    },

    /// Display or modify configuration.
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Generate shell completion scripts.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(clap::Args, Debug)]
pub struct RestoreArgs {
    /// Restore to this directory instead of the original location.
    #[arg(long, value_name = "PATH")]
    pub destination: Option<PathBuf>,
    /// Restrict the restore to one managed directory.
    #[arg(long, value_name = "ROOT")]
    pub root: Option<String>,
    /// Restore only these paths (relative, repeatable).
    #[arg(long = "path", value_name = "REL")]
    pub paths: Vec<String>,
    /// Replace files that already exist.
    #[arg(long, conflicts_with_all = ["skip", "rename"])]
    pub overwrite: bool,
    /// Leave existing files untouched.
    #[arg(long)]
    pub skip: bool,
    /// Write alongside existing files with a "(restored)" suffix.
    #[arg(long)]
    pub rename: bool,
    /// Analyse only; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print a single configuration value.
    Get { key: String },
    /// Set a configuration value.
    Set { key: String, value: String },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl From<Shell> for clap_complete::Shell {
    fn from(s: Shell) -> Self {
        match s {
            Shell::Bash => clap_complete::Shell::Bash,
            Shell::Zsh => clap_complete::Shell::Zsh,
            Shell::Fish => clap_complete::Shell::Fish,
        }
    }
}
