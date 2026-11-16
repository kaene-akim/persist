//! Progress reporting for the CLI, backed by `indicatif`.

use indicatif::{ProgressBar, ProgressStyle};
use persist_core::engine::Progress;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A global cancellation flag, set by the Ctrl-C handler.
#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A [`Progress`] implementation that draws a progress bar.
pub struct CliProgress {
    bar: ProgressBar,
    cancel: CancelFlag,
    enabled: bool,
}

impl CliProgress {
    pub fn new(cancel: CancelFlag, enabled: bool) -> Self {
        let bar = ProgressBar::new(0);
        if enabled {
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} {msg} [{bar:32.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏  "),
            );
        } else {
            bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }
        Self {
            bar,
            cancel,
            enabled,
        }
    }
}

impl Progress for CliProgress {
    fn phase(&self, name: &str, total: u64) {
        if self.enabled {
            self.bar.set_length(total);
            self.bar.set_position(0);
            self.bar.set_message(name.to_string());
        }
    }

    fn progress(&self, done: u64, total: u64, bytes_done: u64, bytes_total: u64) {
        if self.enabled {
            if total > 0 {
                self.bar.set_length(total);
            }
            self.bar.set_position(done);
            if bytes_total > 0 {
                self.bar.set_message(format!(
                    "{} / {}",
                    persist_core::util::human_size(bytes_done),
                    persist_core::util::human_size(bytes_total)
                ));
            }
        }
    }

    fn message(&self, message: &str) {
        if self.enabled {
            self.bar.set_message(message.to_string());
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Drop for CliProgress {
    fn drop(&mut self) {
        if self.enabled {
            self.bar.finish_and_clear();
        }
    }
}
