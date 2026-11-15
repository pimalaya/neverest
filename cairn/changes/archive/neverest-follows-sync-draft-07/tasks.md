---
cairn: tasks
change: neverest-follows-sync-draft-07
---

# Tasks

- [x] The 2026-10-30 work committed first (user, 2026-11-01)
- [x] `rusqlite = "0.41"`; Cargo.lock moves io-pimdir, rusqlite and libsqlite3-sys alone
- [x] The compile port (proposal's list, then the second pass); `cargo test` green on what it asserted before, every changed assertion named in the log
- [x] Narrowed rounds: `narrowed` and `enumerated` on IMAP, Gmail and Graph scoped rounds; regression tests (an undated draft below the window kept, a `Date` ahead of arrival kept) failing first against ec6603b's behaviour; tests/scope.rs on Stalwart (tests/stalwart.sh) (done on 2026-11-01; tests/scope.rs passed on Stalwart from a port-shifted copy; band rounds state `narrowed` and enumerate nothing, see the log)
- [x] Upgrades take `PimdirUpgradeOptions::from(&sync_options)`; a test where a fetched divergence settles by `prefer-local` and keeps the overruled body
- [x] Report, JSON schema and exit code carry `Overruled`, the empty-listing `parked`, `unanswered`, `rejected_creates`, `unreadable`
- [x] Error vocabulary and messages (proposal item 4, 5); a draft-07 store (built through rusqlite by rewriting `store_meta.format_version`) refused as `store-version` with the reinstall message
- [x] `neverest init` makes a store the readers open (no `store-exposed`), tested with umask 022
- [x] Local suites: Stalwart, Radicale (tests/radicale.sh): 19 targets, 48 tests, from a port-shifted copy (rootless Docker cannot publish :143 and :144), two stale tests fixed (see the log)
- [x] Live suites on the test accounts, run by the user or with their go: `GOOGLE_SERVICE_ACCOUNT_KEY_FILE=<path> cargo test --test google -- --ignored --test-threads=1`; `MSGRAPH_CLIENT_SECRET="$(pass show pimalaya/test/microsoft | head -1)" cargo test --test msgraph -- --ignored --test-threads=1`; Gmail and Graph enumeration cost noted
- [x] Pin to io-pimdir bff5737 by git rev, path patch dropped
- [x] Fold into spec/sync.md (the two scope requirements rewritten, draft citations), MIGRATION.md, CHANGELOG; log; land (folded, MIGRATION.md, CHANGELOG and log written on 2026-11-01; landing left, landed 2026-11-01)
