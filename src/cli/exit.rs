//! # Exit
//!
//! What a command concluded, and the process exit code it becomes.

use std::process::ExitCode;

use crate::sync::report::SyncOutput;

/// How a command ended, beyond the success or failure its `Result` carries.
///
/// Two codes beyond success exist: one for a run that reconciled and left
/// something behind for a person, one for a run that could not do all its
/// work, a rerun picking it up. Failing instead would stop the other ten
/// thousand items over one duplicated phone number, and would loop forever
/// under a supervisor restarting on failure.
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
    /// A sync left work a rerun picks up: a source it could not reach, or
    /// a hunk, a send or an intent that failed without parking.
    ///
    /// It wins over [`Exit::Conflicted`]: the run did not see everything,
    /// and the report still counts what waits for a person.
    Incomplete,
}

impl From<&SyncOutput> for Exit {
    /// A run ends the way its report reads: delivered, or still waiting.
    fn from(report: &SyncOutput) -> Self {
        if report.incomplete() {
            Self::Incomplete
        } else if report.left_waiting() {
            Self::Conflicted
        } else {
            Self::Success
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
            Exit::Incomplete => ExitCode::from(3),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;
    use crate::sync::{
        hunk::CollectionHunk,
        report::{PatchEntry, SubmitEntry, ThrottledSource},
    };

    fn unreachable() -> PatchEntry<CollectionHunk> {
        let scan = CollectionHunk::Scan {
            side: String::from("imap"),
            collection: String::from("*"),
        };
        PatchEntry::new(scan, Some(anyhow!("Open connection: connection refused")))
    }

    /// A run that could not reach a source is not a run with nothing to
    /// do, and it says so even with a conflict waiting.
    #[test]
    fn an_unreachable_source_makes_the_run_incomplete() {
        let mut report = SyncOutput::default();
        assert_eq!(Exit::from(&report), Exit::Success);

        report.collection.patch.push(unreachable());
        assert_eq!(Exit::from(&report), Exit::Incomplete);
        assert_eq!(ExitCode::from(Exit::Incomplete), ExitCode::from(3));

        report.outstanding_conflicts = 1;
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// A send that parked waits for a person, not for a rerun.
    #[test]
    fn a_parked_send_is_not_incomplete() {
        let mut report = SyncOutput::default();
        report.submitted.push(SubmitEntry {
            id: 1,
            collection: String::from("INBOX"),
            subject: None,
            error: Some(String::from("550 rejected")),
            parked: true,
        });
        assert_eq!(Exit::from(&report), Exit::Success);

        report.submitted[0].parked = false;
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// A source that gave up throttled left work for a rerun, even when
    /// every collection it scanned reads as done.
    #[test]
    fn a_throttled_source_makes_the_run_incomplete() {
        let mut report = SyncOutput::default();
        report.throttled.push(ThrottledSource {
            source: String::from("gmail"),
            until: String::from("2026-10-07T10:00:00Z"),
        });
        assert_eq!(Exit::from(&report), Exit::Incomplete);

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json["throttled"],
            serde_json::json!([{ "source": "gmail", "until": "2026-10-07T10:00:00Z" }])
        );
    }

    /// A round left open was interrupted: what landed stays, and a rerun
    /// resumes it, so the run is incomplete; a closed one is coverage only.
    #[test]
    fn an_open_round_makes_the_run_incomplete() {
        use crate::sync::report::{CollectionCoverage, OpenRound, SourceDownload};

        let mut report = SyncOutput::default();
        report.coverage.push(CollectionCoverage {
            source: String::from("imap"),
            collection: String::from("INBOX"),
            since: Some(String::from("2026-09-07T00:00:00Z")),
            until: None,
            at: Some(String::from("2026-10-07T10:00:00Z")),
            round: None,
        });
        report.downloaded.push(SourceDownload {
            source: String::from("imap"),
            bytes: 2048,
        });
        assert_eq!(Exit::from(&report), Exit::Success);

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json["coverage"],
            serde_json::json!([{
                "source": "imap",
                "collection": "INBOX",
                "since": "2026-09-07T00:00:00Z",
                "at": "2026-10-07T10:00:00Z",
            }])
        );
        assert_eq!(
            json["downloaded"],
            serde_json::json!([{ "source": "imap", "bytes": 2048 }])
        );

        report.coverage[0].round = Some(OpenRound {
            since: Some(String::from("2025-10-07T00:00:00Z")),
            until: None,
            started_at: String::from("2026-10-07T10:05:00Z"),
        });
        assert_eq!(Exit::from(&report), Exit::Incomplete);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json["coverage"][0]["round"],
            serde_json::json!({
                "since": "2025-10-07T00:00:00Z",
                "startedAt": "2026-10-07T10:05:00Z",
            })
        );
    }
}
