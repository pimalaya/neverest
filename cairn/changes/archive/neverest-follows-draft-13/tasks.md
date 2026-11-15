---
cairn: tasks
change: neverest-follows-draft-13
---

# Tasks

- [x] io-pimdir pinned by git at d184622, then `io-pimdir = "0.14.0"` from crates.io once published, no `[patch]` left; pimalaya-cli 0.6.0, io-imap 0.11.2, io-gpeople 0.8.1, io-msgraph 0.9.2, io-gcal 0.5.1; the Cargo.lock diff kept to those six crates
- [x] The build needs no port of a `match`: neverest matches no `PimdirPushOutcome`, and its `PimdirSyncEvent` matches end in `_`
- [x] N27: IMAP and Gmail band rounds hand the enumeration on their last page; Graph's band round too, by a fresh delta (`folder_ids`)
- [x] N30: `error::refusal`, `names_the_request`, each protocol's `refuses` (DAV, Gmail, Google Calendar, People, Graph, IMAP); `PimRemote::fail` on every write
- [x] The engine's `Refused` events noted once a run, reported under `refusals`, their hunks taken back, counted as warnings; the `refused` sync event
- [x] The store refusal names the rebuild; the draft-11 store, index and version tests
- [x] Tests: the connectors' mappings over scripted servers, the hold end to end over the fake IMAP server, the band enumerations, the event and the report
- [x] cairn/spec/sync.md, CHANGELOG.md, MIGRATION.md
- [x] Checks: fmt, tests with all features, the reduced CI test, clippy over the 12 CI sets, deny, verify.sh; io-pimdir's vectors_sync at 0.14.0 (format 7, 167 cases)
