---
cairn: tasks
change: a-send-is-handed-over-once
---

# Tasks

- [x] `offline/submit`: `SubmitFailure::Uncertain`, `reason()` under the `uncertain: ` prefix; `smtp_disposition` and `graph_disposition`; `prepare` then `deliver`; the SMTP exchange driven over `SmtpMessageSend` (`hand_over_smtp`), the data's final `.` noted once written and flushed; `RSET` after a refusal, a channel lost after a failure with no reply; `open_failure` (unreached or refused credentials); `InFlight`, the `submitting` mark.
- [x] `msgraph/client`: `send_mime` answering `SendMimeFailure { error, unanswered }` from the session's exchange record, the one way a Graph exchange is driven (`wire::Exchange::written`, set when the request is flushed whole before its first read; cleared at the start of each exchange, before a reopen); a `POST` is never sent again on a dead connection, a 429 alone being sent again; an answer cut short or unparsable counts as none.
- [x] `offline/driver`: `drain_submits` counts the attempt and marks it before the first byte, records the outcome, then clears the mark; `park_interrupted`; `SendSide` (a side with or without its session); `open_send_channel` answering `Unopened` (pending, unreached as `<source>.smtp`, refused); `run_intents` and `wanted_sides` for `--intents-only`, opening an endpoint only for an intent that needs it; `note_source_failure` shared with `run`; no sweep under `--intents-only`.
- [x] `offline/state`: `stamp` clears a mark an earlier store left.
- [x] `sync/report`: `SubmitEntry.uncertain`; `left_waiting` counts an uncertain send (exit 2), `incomplete` an unreached endpoint (exit 3); docs of `unreached`.
- [x] `cli/sync`: `--intents-only`, refusing `--dry-run`, `--reset`, `--declare-only`, `--since` and the collection filters; exit codes documented.
- [x] Tests: unit, the dispositions, the mark, the SMTP sink (a hang-up after and before the data, a reply to the data, `RSET`, refused credentials, a closed port), Graph `sendMail` answered or not (local fake), `wanted_sides`, `stamp`, the exit codes and the `submitted` JSON; tests/intents.rs (no server): a send through SMTP alone with the source closed, an uncertain park, a mark from a stopped run, the attempt and the mark while the server holds its verdict, a run killed mid-send, an unreached SMTP server, refused credentials, a refused filter; Stalwart: tests/submit.rs `intents_only_sends_and_the_next_sync_files_the_copy`, tests/create.rs `a_queued_folder_is_created_by_a_run_performing_intents_alone`.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy --all-targets` (default, all features, reduced sets), `cargo fmt`; Stalwart suites submit, create, drain, relay, roles.
