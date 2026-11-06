---
cairn: tasks
change: neverest-follows-storage-draft-09
---

# Tasks

- [x] Pin io-pimdir 5c18a02, `version = "0.9"`; the build needs no port
- [x] ical-rs 0.7, vcard-rs 0.6, io-msgraph 0.7, io-gcal 0.3, io-gpeople 0.6; the ical-rs and io-msgraph patches dropped
- [x] The old-store tests restamp `storage-draft-08`; `written_earlier` asserts draft-08 earlier and draft-10 newer
- [x] Spec, MIGRATION.md and CHANGELOG.md name the new drafts
- [x] `cargo fmt`, `cargo clippy --all-features --all-targets`, `cargo test --all-features`, the feature combinations, `cargo deny check licenses bans`
- [ ] The Stalwart, Radicale and live suites on the new pin (supervisor)
