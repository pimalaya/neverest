---
cairn: tasks
change: sync-safety-fixes
---

# Tasks

- [x] `msgraph`: `sentDateTime` in `DELTA_SELECT`, `message_date` from it; tests: the date is `sentDateTime`, a row without it has none.
- [x] `imap`: `supports_uid_expunge` (UIDPLUS or IMAP4rev2), `delete_message` by `UID EXPUNGE`, refused before any flag on a server with neither; tests: capability unit test, tests/deletes.rs on Stalwart (a message another client marked `\Deleted` survives).
- [x] `throttle`: `Throttle` (back-off, give-up, block, token bucket), `retry_after`, `google_throttled`; shared through `SourceAccount`; every HTTP backend's `op` goes through it; Graph's batch rounds stop once the source gave up and give up after their last round.
- [x] `sync/report`: `throttled: Vec<ThrottledSource>`, carried by `absorb`, counted by `incomplete`; `run` fills it from `Account::throttled`.
- [x] `gmail`: `UNITS_PER_SECOND` (200), each call's cost in quota units.
- [x] Tests: throttle units (bucket, back-off, retry, give-up and block, Google messages, `Retry-After`), exit 3 and JSON for `throttled`, tests/drain.rs a server answering 503 named under `throttled`.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy --all-targets` (and reduced feature sets), `cargo fmt`.
