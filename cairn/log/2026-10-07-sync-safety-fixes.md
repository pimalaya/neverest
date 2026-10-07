---
cairn: log
change: sync-safety-fixes
date: 2026-10-07
---

# Sync safety fixes

Block 1 of the scoped mail sync plan (pimdir `cairn/changes/scoped-mail-sync`, §8): four fixes that need no pimdir spec change.

## What landed

- src/msgraph/client.rs: the delta `$select` asks for `sentDateTime`, and `message_date` reads it, never `receivedDateTime`; a row without it has no date.
- src/imap: `ImapClient::supports_uid_expunge` (UIDPLUS or IMAP4rev2); `delete_message` stores `\Deleted` then `UID EXPUNGE`s that UID, and on a server with neither fails before storing anything, the push kept as rejected.
- src/throttle.rs: `Throttle`, one per resolved endpoint (`SourceAccount::throttle`), shared by its connections: back-off (1 to 16 s with jitter, five retries; a stated wait over two minutes gives up at once), give-up and block, a token bucket for pacing; `retry_after`, `google_throttled` (429, 503, a 403 `rateLimitExceeded` read from the message, `Retry after <instant>`).
- Every HTTP backend's `op` (Graph, Gmail, Google Calendar and People, DAV) runs through it; Graph's batch rounds read `Retry-After` through it, stop once the source gave up, and give up after their last round.
- src/gmail/client.rs: `UNITS_PER_SECOND` (200) and each call's cost in quota units.
- src/sync/report.rs: `throttled: [{ source, until }]`, carried by `absorb`, making the run incomplete; filled by `run` from `Account::throttled`.
- Tests: unit tests for the Graph date, the capability check, the throttle and the exit code; tests/deletes.rs (Stalwart, ignored by default) fails against the plain `EXPUNGE`; tests/drain.rs `a_throttling_server_is_named_in_the_report` (a local server answering 503, exit 3, `caldav` named).

## Capabilities moved

- sync: a Graph mail is dated by its `Date`; an IMAP delete expunges its own message alone; a throttled provider is waited out, then named; Gmail is paced below its quota (new requirements); an incomplete run counts a source that gave up throttled (modified).
