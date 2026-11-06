//! # Run progress
//!
//! How the driver says where a run is: a spinner on stderr in text mode,
//! [`SyncEvent`]s under `--json`. Text mode prints no event, the spinner
//! being its text form; `--json` draws no spinner, which would interleave
//! with the event lines on a terminal.

use std::sync::atomic::{AtomicUsize, Ordering};

use pimalaya_cli::spinner::Spinner;

use crate::sync::event::SyncEvent;

/// Where a run reports its progress.
#[derive(Clone, Copy, Default)]
pub struct Progress<'a> {
    /// What prints an event, `None` in text mode.
    sink: Option<&'a (dyn Fn(SyncEvent) + Sync)>,
}

impl<'a> Progress<'a> {
    /// Progress as events handed to `sink`, with no spinner.
    pub fn events(sink: &'a (dyn Fn(SyncEvent) + Sync)) -> Self {
        Self { sink: Some(sink) }
    }

    /// Starts a step, drawing its spinner in text mode.
    pub fn start(&self, message: impl Into<String>) -> Step {
        Step(self.sink.is_none().then(|| Spinner::start(message)))
    }

    /// Prints `event` under `--json`.
    ///
    /// Takes a closure so text mode builds no event.
    pub fn emit(&self, event: impl FnOnce() -> SyncEvent) {
        if let Some(sink) = self.sink {
            sink(event());
        }
    }
}

/// A step of the run: its spinner in text mode, nothing under `--json`.
pub struct Step(Option<Spinner>);

impl Step {
    /// Replaces the spinner message.
    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(spinner) = &self.0 {
            spinner.set_message(message);
        }
    }

    /// Ends the step with a success line.
    pub fn success(self, message: impl AsRef<str>) {
        if let Some(spinner) = self.0 {
            spinner.success(message);
        }
    }
}

/// Lets a count through once per percent of its total, so a progress event
/// is printed at most 101 times whatever the count.
///
/// Safe to share between the threads of a pool: each percent passes once,
/// to whichever thread reaches it first. A count of 1 starts a new count, as
/// the next phase of a collection does, and always passes.
#[derive(Default)]
pub struct PercentGate(AtomicUsize);

impl PercentGate {
    /// Whether `done` of `total` reaches a percent no call reached yet.
    pub fn passes(&self, done: usize, total: usize) -> bool {
        let percent = (done * 100).checked_div(total).unwrap_or(100);

        if done == 1 {
            self.0.store(percent + 1, Ordering::Relaxed);
            return true;
        }

        // NOTE: shifted by one, so 0% passes once too.
        self.0.fetch_max(percent + 1, Ordering::Relaxed) <= percent
    }
}

#[cfg(test)]
mod tests {
    use super::PercentGate;

    #[test]
    fn a_percent_passes_once() {
        let gate = PercentGate::default();
        let passed: Vec<usize> = (1..=1000).filter(|&n| gate.passes(n, 1000)).collect();

        assert_eq!(passed.len(), 101);
        assert_eq!(passed.first(), Some(&1));
        assert_eq!(passed.last(), Some(&1000));
        // NOTE: a thread lagging behind a later percent passes nothing.
        assert!(!gate.passes(10, 1000));
    }

    #[test]
    fn a_new_count_passes_again() {
        let gate = PercentGate::default();
        assert!(gate.passes(10, 10));

        assert!(gate.passes(1, 4));
        assert!(gate.passes(2, 4));
        assert!(!gate.passes(2, 4));
    }
}
