---
cairn: log
change: neverest-follows-storage-draft-09
date: 2026-11-06
---

# neverest follows storage-draft-09

Pinned at io-pimdir 5c18a02 (crate 0.9.0, pimdir 8c16bb1: storage-draft-09, sync-draft-08, search-draft-09), by git rev in `[patch.crates-io]`, `io-pimdir = "0.9"`.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir 0.9 built with no source change. neverest answers `WantsLoad` through `PimdirSourceStore::service`, so the delta's `PimdirLoadSelection::Owing` is io-pimdir's alone; neverest's own `load` calls (hydration, pager, tests) keep `All` and `Handles`. It copies no `PimdirSearchClock` nor `PimdirSearchOptions`, which lost `Copy`.
- io-pimdir 0.9 depends on ical-rs 0.7, vcard-rs 0.6 and mime-rs 0.1 from crates.io. Moving neverest's own ical-rs and vcard-rs there meant moving the connectors whose types carry them, all released: ical-rs 0.7, vcard-rs 0.6, io-msgraph 0.7, io-gcal 0.3, io-gpeople 0.6. The build needed no source change either, so the bump is in. The ical-rs (99b9273) and io-msgraph (2ae2637) git patches are dropped, both revs being in the releases. The lock holds one ical-rs, one vcard-rs and one simd-cpu (0.1.0) now; the base64-codec, quoted-printable-codec and simd-cpu git 0.0.1 entries are gone with mime-rs's git rev.
- io-gcal 0.3 pushes an event giving its end as a `DURATION`, which 0.2 refused (`NoEnd`); vcard-rs 0.6 folds an edited or built line over 75 octets.
- The old-store refusal is unchanged: `written_earlier` (src/offline/driver.rs) reads `storage-draft-08` below `FORMAT_VERSION` (`storage-draft-09`), so a store any earlier neverest wrote still reads "The store was written by an earlier neverest and cannot be migrated: sync it with the neverest that wrote it until nothing is queued, then `neverest sync --reset --force -a <account>` moves it aside, deleting nothing, and syncs a new one (see MIGRATION.md)".
- Tests: tests/drain.rs `a_store_an_earlier_neverest_wrote_asks_for_a_reinstall` restamps `storage-draft-08`, the store this move strands; src/offline/driver.rs `a_store_version_tells_an_earlier_neverest_from_a_newer_one` asserts draft-08 earlier and draft-10 newer (draft-09 is no longer refused for its version).
- Docs: cairn/spec/sync.md (the drafts in the header, the outgrew scenario), MIGRATION.md (the drafts), CHANGELOG.md (the bumps).

## Evidence

`cargo test --all-features` 506 passed, 68 ignored; `cargo clippy --all-features --all-targets` clean; `cargo check` with no default features, with `search` alone, with `rustls-ring,imap` and with the defaults; `cargo deny check licenses bans` ok (hashlink, foldhash under Zlib, already allowed).

## Not done

- The Stalwart and Radicale suites and the live suites (google.rs, msgraph.rs) on the new pin: the supervisor runs them.
- neverest installs no CPU token for the SIMD codecs (`simd_cpu::SimdCpu::install_detected`), so mime-rs, base64-codec and quoted-printable-codec run their portable baseline in it.
- io-gmail 0.5 and io-pim-discovery 0.9 still depend on io-http 0.5, so the io-http git patch stays and the lock holds io-http 0.5 and 0.6.

## Capabilities moved

- sync (cairn/spec/sync.md): "A store the format outgrew is refused with its remedy" (its scenario), the drafts the spec cites.
