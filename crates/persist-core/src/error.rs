//! Error types shared across the Persist core.

use std::path::PathBuf;

/// Convenient result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All errors that the Persist core can produce.
///
/// The variants are deliberately fine-grained so that callers (the CLI and the
/// desktop application) can react to them and choose meaningful exit codes.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("network error: {0}")]
    Network(String),

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("authentication required: {0}")]
    AuthRequired(String),

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("the stored credentials have expired and could not be refreshed")]
    TokenExpired,

    #[error("not connected to a cloud account")]
    NotConnected,

    #[error("Google Drive API error ({status}): {message}")]
    Drive { status: u16, message: String },

    #[error("rate limited by the storage provider; retry after {retry_after_secs:?}s")]
    RateLimited { retry_after_secs: Option<u64> },

    #[error("cloud storage is full (need {needed} bytes, {available} bytes available)")]
    InsufficientStorage { needed: u64, available: u64 },

    #[error("local disk is full while writing {path}")]
    LocalDiskFull { path: PathBuf },

    #[error("repository not found: {0}")]
    RepositoryNotFound(String),

    #[error("repository is corrupt or has an unsupported format: {0}")]
    InvalidRepository(String),

    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("object {0} is missing from the repository")]
    MissingObject(String),

    #[error("object {hash} is corrupt: expected sha256 {expected}, got {actual}")]
    CorruptObject {
        hash: String,
        expected: String,
        actual: String,
    },

    #[error("path escapes the destination directory (path traversal attempt): {0}")]
    PathTraversal(String),

    #[error("refusing to overwrite existing file: {0}")]
    RefuseOverwrite(PathBuf),

    #[error("directory does not exist: {0}")]
    DirectoryMissing(PathBuf),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("backup root not found: {0}")]
    RootNotFound(String),

    #[error("operation was interrupted")]
    Interrupted,

    #[error("operation cancelled by the user")]
    Cancelled,

    #[error("{0}")]
    Message(String),
}

impl Error {
    /// A stable, machine readable code for this error, suitable for JSON output.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::Serde(_) => "serde",
            Error::Database(_) => "database",
            Error::Network(_) => "network",
            Error::Http(_) => "http",
            Error::AuthRequired(_) => "auth_required",
            Error::AuthFailed(_) => "auth_failed",
            Error::TokenExpired => "token_expired",
            Error::NotConnected => "not_connected",
            Error::Drive { .. } => "drive",
            Error::RateLimited { .. } => "rate_limited",
            Error::InsufficientStorage { .. } => "insufficient_storage",
            Error::LocalDiskFull { .. } => "local_disk_full",
            Error::RepositoryNotFound(_) => "repository_not_found",
            Error::InvalidRepository(_) => "invalid_repository",
            Error::SnapshotNotFound(_) => "snapshot_not_found",
            Error::MissingObject(_) => "missing_object",
            Error::CorruptObject { .. } => "corrupt_object",
            Error::PathTraversal(_) => "path_traversal",
            Error::RefuseOverwrite(_) => "refuse_overwrite",
            Error::DirectoryMissing(_) => "directory_missing",
            Error::PermissionDenied(_) => "permission_denied",
            Error::Config(_) => "config",
            Error::RootNotFound(_) => "root_not_found",
            Error::Interrupted => "interrupted",
            Error::Cancelled => "cancelled",
            Error::Message(_) => "error",
        }
    }

    /// Process exit code the CLI should use for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::AuthRequired(_) | Error::AuthFailed(_) | Error::TokenExpired | Error::NotConnected => 3,
            Error::InsufficientStorage { .. } => 4,
            Error::RepositoryNotFound(_)
            | Error::SnapshotNotFound(_)
            | Error::RootNotFound(_)
            | Error::DirectoryMissing(_) => 5,
            Error::MissingObject(_) | Error::CorruptObject { .. } | Error::InvalidRepository(_) => 6,
            Error::PathTraversal(_) | Error::RefuseOverwrite(_) => 7,
            Error::Cancelled => 130,
            Error::Interrupted => 131,
            _ => 1,
        }
    }

    /// True when retrying the operation might succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::Network(_) | Error::Http(_) | Error::RateLimited { .. }
        )
    }

    pub fn permission(path: impl std::fmt::Display) -> Self {
        Error::PermissionDenied(path.to_string())
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Message(value.to_string())
    }
}
