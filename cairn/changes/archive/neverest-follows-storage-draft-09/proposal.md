---
cairn: change
id: neverest-follows-storage-draft-09
status: landed
created: 2026-11-06
---

# neverest follows storage-draft-09

## Why

io-pimdir 5c18a02 (pushed, master, crate 0.9.0) follows pimdir 8c16bb1: storage-draft-09, sync-draft-08 and search-draft-09 (io-pimdir cairn/log/2026-11-05-the-crate-follows-storage-draft-09.md). A placement is keyed by an integer id and its system flags are bits of the row, so a store draft-08 wrote is refused by every profile, never migrated. A delta loads what owes a push (`PimdirLoadSelection::Owing`) rather than the whole collection.

## What

- Cargo.toml: `io-pimdir = "0.9"`, the `[patch.crates-io]` rev moved to 5c18a026b9c85b470984192452c2aa66e060a3fe.
- io-pimdir 0.9 brings ical-rs 0.7, vcard-rs 0.6 and mime-rs 0.1 from crates.io. neverest's own ical-rs and vcard-rs, and the connectors passing their types (io-msgraph, io-gcal, io-gpeople), move to the same versions when the bump costs no code: ical-rs 0.7, vcard-rs 0.6, io-msgraph 0.7, io-gcal 0.3, io-gpeople 0.6, dropping the ical-rs and io-msgraph git patches their releases contain.
- The old-store refusal: `written_earlier` compares draft numbers, so a draft-08 store reads as an earlier neverest's and gets the reinstall message unchanged (sync it with the neverest that wrote it until nothing is queued, then `sync --reset --force`). Tests restamp `storage-draft-08`, the store this move strands.
- Docs: the drafts named in cairn/spec/sync.md and MIGRATION.md, CHANGELOG.md.

neverest answers `WantsLoad` through `PimdirSourceStore::service`, so `Owing` needs nothing of its own; its direct `load` calls keep `All` and `Handles`. It copies no `PimdirSearchClock` or `PimdirSearchOptions`.
