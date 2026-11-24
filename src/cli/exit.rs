//! # Exit
//!
//! What a command concluded, and the process exit code it becomes.

use std::process::ExitCode;

use pimalaya_cli::error::EXIT_PARTIAL;

use crate::sync::report::{SyncOutput, SyncRunOutput};

/// How a command ended, beyond the success or failure its `Result` carries.
///
/// 3 for a run that left something for a person, 4 for one that could not
/// do all its work. Failing instead would stop ten thousand items over one
/// duplicated phone number, and loop under a supervisor restarting on it.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
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

impl From<&SyncRunOutput> for Exit {
    /// A run ends the way its furthest account did, an account that failed
    /// or found its store busy leaving it incomplete.
    fn from(run: &SyncRunOutput) -> Self {
        run.accounts
            .iter()
            .map(|account| match &account.report {
                Some(report) => Self::from(report),
                None => Self::Incomplete,
            })
            .max()
            .unwrap_or_default()
    }
}

/// Exits 3 or 4: 1 is a failed command, which exits through
/// `ErrorReport::eval` before this runs, and 2 a refused command line.
impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        match exit {
            Exit::Success => ExitCode::SUCCESS,
            Exit::Conflicted => ExitCode::from(EXIT_PARTIAL as u8),
            Exit::Incomplete => ExitCode::from(EXIT_PARTIAL as u8 + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;
    use crate::{
        error::Failure,
        sync::{
            hunk::CollectionHunk,
            report::{
                DrainedQueue, IntentEntry, ParkedQueueAction, SubmitEntry, SubmitFailure,
                ThrottledSource,
            },
        },
    };

    fn unreachable(report: &mut SyncOutput) {
        let scan = CollectionHunk::Scan {
            side: String::from("imap"),
            collection: String::from("*"),
        };
        let err = anyhow!("Open connection: connection refused");
        report.note_collection_hunk(scan, Some(err));
    }

    /// A send attempt of the row `id` that failed, parked or not.
    fn failed_send(id: i64, error: &str, parked: bool, unconfirmed: bool) -> SyncOutput {
        let mut report = SyncOutput::default();
        let entry = SubmitEntry {
            id,
            collection: String::from("imap/Sent"),
            subject: Some(String::from("hi")),
        };
        let failure = SubmitFailure {
            failure: Failure::new(error),
            parked,
            unconfirmed,
        };
        report.note_submitted(entry, Some(failure));
        report
    }

    /// A run that could not reach a source is not a run with nothing to
    /// do, and it says so even with a conflict waiting.
    #[test]
    fn an_unreachable_source_makes_the_run_incomplete() {
        let mut report = SyncOutput::default();
        assert_eq!(Exit::from(&report), Exit::Success);

        unreachable(&mut report);
        assert_eq!(Exit::from(&report), Exit::Incomplete);
        assert_eq!(ExitCode::from(Exit::Incomplete), ExitCode::from(4));

        report.outstanding_conflicts = 1;
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// An incomplete run never reads as in sync in the text, and names what
    /// it did not reach.
    #[test]
    fn an_incomplete_run_says_so_in_the_text() {
        let mut report = SyncOutput {
            account: String::from("dav"),
            interrupted: true,
            ..SyncOutput::default()
        };
        let text = report.to_string();
        assert!(!text.contains("already in sync"), "{text}");
        assert!(text.contains("Account dav is incomplete"), "{text}");

        report.interrupted = false;
        report.throttled.push(ThrottledSource {
            source: String::from("gcal"),
            until: String::from("2026-11-17T18:00:00Z"),
        });
        report.note_unreached(
            String::from("imap.smtp"),
            Failure::of(&anyhow!("connection refused")),
        );
        let text = report.to_string();
        assert!(text.contains("Throttled (1):"), "{text}");
        assert!(text.contains("gcal gave up throttled"), "{text}");
        assert!(text.contains("Unreached (1):"), "{text}");
        assert!(text.contains("imap.smtp: connection refused"), "{text}");
        assert!(text.contains("Account dav is incomplete"), "{text}");
    }

    /// A send the server refused for good parked: the message was not sent,
    /// which waits for a person, not for a rerun, and is no success.
    #[test]
    fn a_parked_send_waits_for_a_person() {
        let report = failed_send(1, "550 rejected", true, false);
        assert_eq!(Exit::from(&report), Exit::Conflicted);

        let report = failed_send(1, "550 rejected", false, false);
        assert_eq!(Exit::from(&report), Exit::Incomplete);
    }

    /// The errors a text report counts are the ones it lists: a send left
    /// pending once printed `Errors (1):` over an empty list.
    #[test]
    fn the_errors_counted_are_listed() {
        let mut report = failed_send(7, "452 4.4.5 rate limit", false, false);
        report.account = String::from("stalwart");
        let text = report.to_string();

        let listed = text
            .split("Errors (1):\n")
            .nth(1)
            .unwrap_or_else(|| panic!("no error section: {text}"));
        let first = listed.lines().next().unwrap_or_default();
        assert!(first.starts_with(" - "), "{text}");
        assert!(first.contains("452 4.4.5 rate limit"), "{text}");
    }

    /// A dry run says what it would apply of the queue: its drain runs on the
    /// replica, and the real queue keeps every row.
    #[test]
    fn a_dry_run_would_apply_the_queue() {
        let mut report = SyncOutput {
            account: String::from("stalwart"),
            dry_run: true,
            ..SyncOutput::default()
        };
        report.drained.push(DrainedQueue {
            collection: String::from("stalwart/imap/INBOX"),
            applied: 2,
        });

        let text = report.to_string();
        assert!(
            text.contains(" - would apply 2 queued action(s) in stalwart/imap/INBOX"),
            "{text}"
        );
        assert!(!text.contains("applied 2"), "{text}");

        report.dry_run = false;
        let text = report.to_string();
        assert!(
            text.contains(" - applied 2 queued action(s) in stalwart/imap/INBOX"),
            "{text}"
        );
    }

    /// A parked intent and a parked queue action wait for a person too.
    #[test]
    fn a_parked_intent_or_queue_action_waits_for_a_person() {
        let mut report = SyncOutput::default();
        let entry = IntentEntry {
            id: 2,
            collection: String::from("caldav/work"),
            kind: String::from("calendar-reply"),
            source: String::from("caldav"),
            seq: None,
        };
        report.note_intent(entry, Some((Failure::new("refused"), true)));
        assert_eq!(Exit::from(&report), Exit::Conflicted);

        let mut report = SyncOutput::default();
        report.parked.push(ParkedQueueAction {
            row: 3,
            handle: String::from("s1.1.3"),
            kind: String::from("remove"),
            collection: String::from("imap/INBOX"),
            link_id: None,
            item_id: Some(String::from("9")),
            producer: String::from("himalaya"),
            queued_at: String::from("2026-11-15T08:00:00.000Z"),
            attempts: 1,
            unconfirmed: false,
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
    fn an_unconfirmed_send_waits_for_a_person() {
        let report = failed_send(7, "unconfirmed: Reached unexpected EOF", true, true);
        assert_eq!(Exit::from(&report), Exit::Conflicted);
        assert_eq!(ExitCode::from(Exit::Conflicted), ExitCode::from(3));

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["submitted"], serde_json::json!([]));
        assert_eq!(
            json["errors"],
            serde_json::json!([{
                "id": null,
                "code": null,
                "error": "unconfirmed: Reached unexpected EOF",
                "sources": [],
                "kind": "submitted",
                "row": 7,
                "collection": "imap/Sent",
                "subject": "hi",
                "parked": true,
                "unconfirmed": true,
            }])
        );
    }

    /// A send channel that did not open has no collection to scan, and
    /// still leaves the run incomplete.
    #[test]
    fn an_unreached_send_channel_makes_the_run_incomplete() {
        let mut report = SyncOutput::default();
        report.note_unreached(
            String::from("imap.smtp"),
            Failure::new("Cannot connect to the SMTP submission server: refused"),
        );
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
            closed_at: Some(String::from("2026-10-07T10:00:00Z")),
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
                "until": null,
                "closedAt": "2026-10-07T10:00:00Z",
                "round": null,
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
                "until": null,
                "startedAt": "2026-10-07T10:05:00Z",
            })
        );
    }

    /// A parked mass absence waits for a person, a rerun alone parking it.
    ///
    /// The round it left open does not make the run incomplete; another
    /// open round still does.
    #[test]
    fn a_parked_absence_waits_for_a_person() {
        use crate::sync::report::{CollectionCoverage, OpenRound, ParkedAbsence};

        let open = |collection: &str| CollectionCoverage {
            source: String::from("imap"),
            collection: String::from(collection),
            since: None,
            until: None,
            closed_at: None,
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
                "side": null,
                "collection": "work",
                "id": null,
                "row": 7,
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

    /// An open series read from a later floor is a warning naming the series
    /// and the floor, and waits for no one: a rerun reads it the same way.
    #[test]
    fn a_clipped_series_is_a_warning_and_changes_no_exit_code() {
        use crate::sync::report::ClippedSeries;

        let mut report = SyncOutput::default();
        report.clipped.push(ClippedSeries {
            side: String::from("msgraph-calendar"),
            collection: String::from("cal"),
            id: String::from("S"),
            since: String::from("2021-11-19T00:00:00Z"),
        });

        assert_eq!(Exit::from(&report), Exit::Success);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["clipped"],
            serde_json::json!([{
                "side": "msgraph-calendar",
                "collection": "cal",
                "id": "S",
                "since": "2021-11-19T00:00:00Z",
            }])
        );
        let text = report.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(
            text.contains("series S in cal on msgraph-calendar starts before 2021-11-19T00:00:00Z"),
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

    /// A run a termination signal stopped did not see everything: it
    /// exits 4, as any incomplete run, and says so.
    #[test]
    fn an_interrupted_run_is_incomplete() {
        let report = SyncOutput {
            interrupted: true,
            ..Default::default()
        };

        assert_eq!(Exit::from(&report), Exit::Incomplete);
        assert!(report.to_string().contains("Interrupted"), "{report}");
    }
}
