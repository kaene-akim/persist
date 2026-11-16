//! Shared application state for the Tauri backend.

use persist_core::session::Session;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The state managed by Tauri and handed to every command.
pub struct AppState {
    /// The core session. A single mutex serialises access because several
    /// operations (authentication, repository init) take `&mut Session`, and
    /// because concurrent backups against the same repository would be unsafe.
    pub session: Mutex<Session>,
    /// Cooperative cancellation flag shared with the active operation.
    pub cancel: Arc<AtomicBool>,
}

impl AppState {
    /// Open the session using the standard Persist locations.
    pub fn new() -> persist_core::Result<Self> {
        Ok(Self {
            session: Mutex::new(Session::open_default()?),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
}
