//! The backup engine.
//!
//! The engine is deliberately independent of both interfaces (CLI and GUI) and
//! of the storage provider. It is driven by the [`crate::repo::Repository`] and
//! reports progress through the [`Progress`] trait.

pub mod backup;
pub mod diff;
pub mod restore;
pub mod verify;

pub use backup::{execute_backup, plan_backup, BackupOptions};
pub use restore::{plan_restore, restore, OverwritePolicy, RestoreOptions, RestorePlan};
pub use verify::{verify_repository, VerifyReport};

/// Progress reporting sink used by long running operations. Implementations must
/// be cheap to call and thread safe.
pub trait Progress: Send + Sync {
    /// A new phase started. `total` is the number of work units, or 0 if unknown.
    fn phase(&self, name: &str, total: u64) {
        let _ = (name, total);
    }
    /// Progress within the current phase.
    fn progress(&self, done: u64, total: u64, bytes_done: u64, bytes_total: u64) {
        let _ = (done, total, bytes_done, bytes_total);
    }
    /// A human readable status message.
    fn message(&self, message: &str) {
        let _ = message;
    }
    /// True when the user has requested cancellation.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A [`Progress`] implementation that discards everything.
pub struct NoopProgress;
impl Progress for NoopProgress {}

/// A [`Progress`] implementation driven by two closures, convenient for tests
/// and for the CLI.
pub struct FnProgress<F, G>
where
    F: Fn(u64, u64, u64, u64) + Send + Sync,
    G: Fn(&str) + Send + Sync,
{
    pub on_progress: F,
    pub on_message: G,
}

impl<F, G> Progress for FnProgress<F, G>
where
    F: Fn(u64, u64, u64, u64) + Send + Sync,
    G: Fn(&str) + Send + Sync,
{
    fn progress(&self, done: u64, total: u64, bytes_done: u64, bytes_total: u64) {
        (self.on_progress)(done, total, bytes_done, bytes_total);
    }
    fn message(&self, message: &str) {
        (self.on_message)(message);
    }
}
