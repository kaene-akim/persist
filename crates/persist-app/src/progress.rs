//! A [`Progress`] implementation that forwards engine progress to the frontend
//! as Tauri events, and honours a shared cancellation flag.

use persist_core::engine::Progress;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// One progress update sent to the webview.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressEvent {
    Phase {
        phase: String,
        total: u64,
    },
    Progress {
        done: u64,
        total: u64,
        bytes_done: u64,
        bytes_total: u64,
    },
    Message {
        message: String,
    },
}

/// Forwards progress to `event` on the given app handle.
pub struct EventProgress {
    app: AppHandle,
    event: String,
    cancel: Arc<AtomicBool>,
}

impl EventProgress {
    pub fn new(app: AppHandle, event: impl Into<String>, cancel: Arc<AtomicBool>) -> Self {
        Self {
            app,
            event: event.into(),
            cancel,
        }
    }

    fn emit(&self, payload: ProgressEvent) {
        let _ = self.app.emit(&self.event, payload);
    }
}

impl Progress for EventProgress {
    fn phase(&self, name: &str, total: u64) {
        self.emit(ProgressEvent::Phase {
            phase: name.to_string(),
            total,
        });
    }

    fn progress(&self, done: u64, total: u64, bytes_done: u64, bytes_total: u64) {
        self.emit(ProgressEvent::Progress {
            done,
            total,
            bytes_done,
            bytes_total,
        });
    }

    fn message(&self, message: &str) {
        self.emit(ProgressEvent::Message {
            message: message.to_string(),
        });
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}
