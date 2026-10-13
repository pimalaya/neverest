---
cairn: log
change: a-send-is-handed-over-once
date: 2026-10-13
---

# A send is handed over once, and can run alone

`sync --intents-only` drains the queue and performs the queued intents (collection creations, sends, invitation replies and cancellations) and stops, listing nothing, opening an endpoint only for an intent that needs it: a send through an `smtp` table opens that channel alone. A send failing after the message was handed over whole with no verdict (SMTP past the final `.`, Graph written whole with no answer or answered 500, 502, 503 or 504) is uncertain: parked under `uncertain: `, reported with `uncertain: true` in `submitted`, the run exiting 2, never sent again. Each send's attempt is counted and its queue id marked in `submitting` before the first byte leaves, so a run killed mid-send leaves the row attempted and the next run parks it instead of sending it twice. An SMTP server that cannot be reached is named `<source>.smtp` under `unreached` (exit 3); refused credentials fail each send with the reason, unparked. Reported by MOA, whose send ran a whole sync of Sent and could not tell a message never handed over from one maybe delivered.

## What landed

- src/offline/submit.rs: `SubmitFailure::Uncertain`, `smtp_disposition`, `graph_disposition`, `prepare` and `deliver`, the SMTP exchange driven over `SmtpMessageSend` (`hand_over_smtp`), `RSET` after a refusal and a lost channel after a failure with no reply, `open_failure`, `InFlight`.
- src/msgraph/client.rs, src/msgraph/client/wire.rs: `send_mime` answering `SendMimeFailure` through the connection record of the Graph session (`Exchange::written`: the request flushed whole before its first read); the record cleared before a reopen.
- src/offline/driver.rs: `drain_submits` (attempt and mark first, outcome, mark cleared), `park_interrupted`, `SendSide`, `open_send_channel` answering `Unopened`, `run_intents`, `wanted_sides`, `note_source_failure`.
- src/offline/state.rs: `stamp` clears a stale mark. src/sync/report.rs: `SubmitEntry.uncertain`, exit 2 for an uncertain send, exit 3 for any `unreached`. src/cli/sync.rs: `--intents-only`.
- tests/intents.rs (no server), tests/submit.rs and tests/create.rs (Stalwart).

## Capabilities moved

sync: added "A run may perform the queued intents alone" and "A send that may have been delivered is never sent again"; modified "A queued submission is a `submit` queue intent" (at-most-once, uncertain, the SMTP channel reported), "An unreached endpoint is named in the report" (`<source>.smtp`), "A conflicted run succeeds, with its own exit code" (an uncertain send), "An incomplete run has its own exit code" (a send channel) and "Neverest is the store's sole owner and drains the queue first" (`--intents-only` drains).
