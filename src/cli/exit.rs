//! What a command concluded, and the process exit code it becomes.

use std::process::ExitCode;

use crate::sync::report::SyncReport;

/// How a command ended, beyond the success or failure its `Result` already
/// carries.
///
/// One code beyond success exists, and it is for a run that reconciled its
/// collections and left something behind. That is one item wide and halts
/// nothing: failing the run would stop the other ten thousand items over one
/// duplicated phone number, and under a supervisor restarting on failure it
/// would loop over a state no supervisor can fix. A code of its own says the
/// same thing to a caller without the run pretending to break.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Exit {
    /// The command did what it was asked and left nothing waiting.
    #[default]
    Success,
    /// A sync reconciled its collections and left work a person has to
    /// settle: a parked conflict, a duplicate `UID` a side refuses, or a
    /// write it would not take. All three are the same answer to a caller,
    /// namely that a rerun on its own changes nothing.
    Conflicted,
}

impl From<&SyncReport> for Exit {
    /// A run ends the way its report reads: everything delivered, or
    /// something still waiting for a person.
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
            // NOTE: 1 belongs to a failed command, which never reaches here:
            // `ErrorReport::eval` exits with it before this conversion runs.
            Exit::Conflicted => ExitCode::from(2),
        }
    }
}
