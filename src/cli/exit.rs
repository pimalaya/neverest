//! # Exit
//!
//! What a command concluded, and the process exit code it becomes.

use std::process::ExitCode;

use crate::sync::report::SyncReport;

/// How a command ended, beyond the success or failure its `Result` carries.
///
/// One code beyond success exists, for a run that reconciled and left
/// something behind. Failing instead would stop the other ten thousand items
/// over one duplicated phone number, and would loop forever under a
/// supervisor restarting on failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Exit {
    /// The command did what it was asked and left nothing waiting.
    #[default]
    Success,
    /// A sync left work a person has to settle.
    ///
    /// A parked conflict, a duplicate `UID` a side refuses, or a write it
    /// would not take: all three say a rerun on its own changes nothing.
    Conflicted,
}

impl From<&SyncReport> for Exit {
    /// A run ends the way its report reads: delivered, or still waiting.
    fn from(report: &SyncReport) -> Self {
        match report.left_waiting() {
            false => Self::Success,
            true => Self::Conflicted,
        }
    }
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        match exit {
            Exit::Success => ExitCode::SUCCESS,
            // NOTE: 1 belongs to a failed command, which exits through
            // `ErrorReport::eval` before this conversion runs.
            Exit::Conflicted => ExitCode::from(2),
        }
    }
}
