---
cairn: log
change: the-server-suites-run-on-head
date: 2026-11-08
---

# The server suites run on HEAD

The Stalwart and Radicale suites had not run since 2026-11-01: five landings since (rename and delete intents, the freeze gaps, progress events, every key printed, storage-draft-09) left them to the supervisor. On d07f80b they ran 43 of 49; the six failures were all in the tests, the reports they printed showing the behaviour the spec states.

## What landed

- tests/endpoints.rs (three tests) asserted a report holds no `conflicts`, `refused` or `rejected` key, which every-key-is-printed (2026-11-03) prints as `[]`; they now read the result's lists (`none` in tests/common). tests/submit.rs read the check's send channel as `"smtp":true`, a row since a-check-reports-every-endpoint, and counted a submission by its `submitted` key, now always there: it reads `sources[0].smtp.ok` and one `submitted` entry with no error. tests/drain.rs parsed the whole output of a sync as one document; it reads the last, after the events.
- tests/create.rs `a_queued_folder_is_renamed_then_deleted_on_the_imap_server`, never run before: its runs, narrowed to the parent, never listed the child into the store, which the spec scenario's GIVEN holds; it now syncs the child once before the rename.
- The `pimdir` CLI the store-reading suites spawn (caldav, calendars, carddav, endpoints, submit) is the one tests/pimdir.sh installs from the io-pimdir commit Cargo.lock locks, under target/pimdir/<commit>/, through `pimdir_cli` in tests/common, no longer one built from ../io-pimdir.
- The server scripts take their host ports from the environment (defaults unchanged), and tests/pasta.sh runs the suites under a rootless Docker: the servers on shifted ports, the command in a pasta network namespace forwarding the ports the suites name. The scratch copy with sed-shifted ports is no longer needed.
- tests/google.rs asks `calendar.events`, `calendar.calendarlist.readonly` and `calendar.calendars` (the throwaway calendars) instead of `calendar`, which the service account's delegation refuses since the 2026-11-07 narrowing (the live job of d07f80b failed on it). `google_token` takes a scope list.

## Verified

- The 19 server suites through tests/pasta.sh: 49 passed. `cargo test --all-features`: 506 passed, 68 ignored. `cargo clippy --all-features --all-targets -- -D warnings` clean.
- tests/google.rs live on the test account with the narrowed scopes: 6 passed. tests/msgraph.rs not run.

## Left

- The server suites still run in no CI job.
- Found reading the Android app's 2026-11-06 and 2026-11-07 sync fixes against this code, not reproduced: an SMTP 5xx always parks a send for good, a daily quota answered `550 5.4.5` included (src/offline/submit.rs `smtp_disposition`); the Sent copy staged for an IMAP source carries `\Seen` (src/offline/driver.rs), which landing on the provider's own copy would push, as the app's sent-copy-takes-provider-markers found on Gmail; under a date scope, a copy whose answer was lost is pushed again, io-pimdir holding back only a create without an origin outside the scope (io-pimdir src/sync.rs).

No capability moved.
