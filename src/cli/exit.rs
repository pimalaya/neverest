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
    /// A parked conflict, a duplicate `UID` a side refuses, a write it would
    /// not take, or a send that may have been delivered: all four say a
    /// rerun on its own changes nothing.
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
        report::{
            IntentEntry, ParkedQueueAction, PatchEntry, SubmitEntry, ThrottledSource,
            UnreachedEndpoint,
        },
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

    /// A send the server refused for good parked: the message was not sent,
    /// which waits for a person, not for a rerun, and is no success.
    #[test]
    fn a_parked_send_waits_for_a_person() {
        let mut report = SyncOutput::default();
        report.submitted.push(SubmitEntry {
            id: 1,
            collection: String::from("INBOX"),
            subject: None,
            error: Some(String::from("550 rejected")),
            parked: true,
            uncertain: false,
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);

        report.submitted[0].parked = false;
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// A parked intent and a parked queue action wait for a person too.
    #[test]
    fn a_parked_intent_or_queue_action_waits_for_a_person() {
        let mut report = SyncOutput::default();
        report.intents.push(IntentEntry {
            id: 2,
            collection: String::from("caldav/work"),
            kind: String::from("calendar-reply"),
            source: String::from("caldav"),
            seq: None,
            error: Some(String::from("refused")),
            parked: true,
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);

        let mut report = SyncOutput::default();
        report.parked.push(ParkedQueueAction {
            id: 3,
            collection: String::from("imap/INBOX"),
            action: String::from("remove"),
            producer: String::from("himalaya"),
            error: String::from("unknown seq 9"),
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);
    }

    /// A run whose convergence passes ended still moving stopped before
    /// its collections settled: a rerun goes on.
    #[test]
    fn an_unsettled_run_is_incomplete() {
        let mut report = SyncOutput::default();
        report.unsettled.push(String::from("imap/INBOX"));
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// A send that may have been delivered is parked for a person to
    /// decide, never resent: the run left something waiting.
    #[test]
    fn an_uncertain_send_waits_for_a_person() {
        let mut report = SyncOutput::default();
        report.submitted.push(SubmitEntry {
            id: 7,
            collection: String::from("imap/Sent"),
            subject: Some(String::from("hi")),
            error: Some(String::from("uncertain: Reached unexpected EOF")),
            parked: true,
            uncertain: true,
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);
        assert_eq!(ExitCode::from(Exit::Conflicted), ExitCode::from(2));

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json["submitted"],
            serde_json::json!([{
                "id": 7,
                "collection": "imap/Sent",
                "subject": "hi",
                "error": "uncertain: Reached unexpected EOF",
                "parked": true,
                "uncertain": true,
            }])
        );
    }

    /// A send channel that did not open has no collection to scan, and
    /// still leaves the run incomplete.
    #[test]
    fn an_unreached_send_channel_makes_the_run_incomplete() {
        let mut report = SyncOutput::default();
        report.unreached.push(UnreachedEndpoint {
            endpoint: String::from("imap.smtp"),
            error: String::from("Cannot connect to the SMTP submission server: refused"),
        });
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

    /// A listing that parked a mass absence waits for a person to confirm
    /// it, a rerun alone parking again: the round it left open makes the
    /// run no less complete, another one still does.
    #[test]
    fn a_parked_absence_waits_for_a_person() {
        use crate::sync::report::{CollectionCoverage, OpenRound, ParkedAbsence};

        let open = |collection: &str| CollectionCoverage {
            source: String::from("imap"),
            collection: String::from(collection),
            since: None,
            until: None,
            at: None,
            round: Some(OpenRound {
                since: None,
                until: None,
                started_at: String::from("2026-10-30T10:00:00Z"),
            }),
        };
        let mut report = SyncOutput::default();
        report.coverage.push(open("INBOX"));
        report.absences.push(ParkedAbsence {
            side: String::from("imap"),
            collection: String::from("INBOX"),
            absent: 24,
            bound: 25,
            rebuild: false,
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["absences"],
            serde_json::json!([{
                "side": "imap",
                "collection": "INBOX",
                "absent": 24,
                "bound": 25,
                "rebuild": false,
            }])
        );
        assert!(report.to_string().contains(
            "the listing of INBOX on imap left out 24 of 25 items, so nothing was dropped"
        ));

        report.coverage.push(open("Archive"));
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// A change withheld over a body the store cannot serve waits for a
    /// person; a body a policy overruled is reported and waits for none.
    #[test]
    fn an_unreadable_body_waits_and_an_overruled_one_does_not() {
        use crate::sync::report::{OverruledBody, UnreadableBodies};

        let mut report = SyncOutput::default();
        report.overruled.push(OverruledBody {
            side: Some(String::from("caldav")),
            collection: String::from("work"),
            id: Some(String::from("e1.ics")),
            action: None,
            kept: String::from("dup:e1@x#e1.ics"),
        });
        assert_eq!(Exit::from(&report), Exit::Success);
        assert!(
            report
                .to_string()
                .contains("kept as retained item dup:e1@x#e1.ics")
        );

        report.unreadable.push(UnreadableBodies {
            side: String::from("caldav"),
            collection: String::from("work"),
            count: 2,
        });
        assert_eq!(Exit::from(&report), Exit::Conflicted);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["unreadable"],
            serde_json::json!([{ "side": "caldav", "collection": "work", "count": 2 }])
        );
    }

    /// A body the drain kept aside applying a queued action is a warning
    /// naming the row, as one a side's write kept is.
    #[test]
    fn a_body_the_drain_overruled_is_named_by_its_row_and_waits_for_none() {
        use crate::sync::report::OverruledBody;

        let mut report = SyncOutput::default();
        report.overruled.push(OverruledBody {
            side: None,
            collection: String::from("work"),
            id: None,
            action: Some(7),
            kept: String::from("dup:e1@x#e1.ics"),
        });

        assert_eq!(Exit::from(&report), Exit::Success);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["overruled"],
            serde_json::json!([{
                "collection": "work",
                "action": 7,
                "kept": "dup:e1@x#e1.ics",
            }])
        );
        let text = report.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(
            text.contains("queued action #7 in work settled by policy"),
            "{text}"
        );
    }

    /// A push its server answered nothing for is pushed again by the next
    /// run, as a hunk that failed is: the run is incomplete.
    #[test]
    fn an_unanswered_push_makes_the_run_incomplete() {
        use crate::sync::report::UnansweredPushes;

        let mut report = SyncOutput::default();
        report.unanswered.push(UnansweredPushes {
            side: String::from("caldav"),
            collection: String::from("work"),
            count: 3,
        });

        assert_eq!(Exit::from(&report), Exit::Incomplete);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["unanswered"],
            serde_json::json!([{ "side": "caldav", "collection": "work", "count": 3 }])
        );
        assert!(report.to_string().contains("got no answer from the server"));
    }

    /// The search index is derived: a refresh failing, busy or cut by its
    /// deadline is reported and changes no exit code.
    #[test]
    fn an_index_refresh_never_changes_the_exit_code() {
        use crate::sync::report::IndexRefresh;

        let mut report = SyncOutput {
            index: Some(IndexRefresh {
                behind: 12,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(Exit::from(&report), Exit::Success);
        assert!(report.to_string().contains("12 change(s) behind"));

        report.index = Some(IndexRefresh {
            error: Some(String::from("disk full")),
            ..Default::default()
        });
        assert_eq!(Exit::from(&report), Exit::Success);
        let text = report.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(
            text.contains("search index not refreshed: disk full"),
            "{text}"
        );
        assert_eq!(
            serde_json::to_value(&report).unwrap()["index"]["error"],
            "disk full"
        );
    }
}
