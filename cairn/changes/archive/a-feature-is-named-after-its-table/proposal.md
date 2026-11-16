---
cairn: change
id: a-feature-is-named-after-its-table
status: landed
created: 2026-11-16
---

# A feature is named after its table

## Why

Line L21 of the CLI alignment round of 2026-11-15, owner ruling D33 of 2026-11-16: a cargo feature matches the configuration table it builds. cardamum's DAV feature is `carddav` and calendula's `caldav`, each its table's name; neverest read both tables under one `dav` feature, so a packager could not build one DAV kind without the other, and the missing-feature refusal of a `caldav` table named `dav`.

## What

- Cargo.toml: `dav` split into `carddav` and `caldav`, both in the default set, no alias `dav` kept (the family has none).
- The DAV adapter (src/dav, the `Dav` account and client arms, the auth conversion, the refusal readers) serves both kinds, under `cfg(any(feature = "carddav", feature = "caldav"))`; opening a `carddav` source is under `carddav`, a `caldav` source under `caldav` (src/account.rs).
- `config::is_built` maps `carddav` and `caldav` to their own feature, and `SourceBackendConfig::feature` is the table's name for every backend, so the `backend-not-built` refusal names `--features carddav` or `--features caldav`.
- CI clippy sets gain `rustls-ring,carddav` and `rustls-ring,caldav` in place of `rustls-ring,dav`; README, CONTRIBUTING, config.sample.toml, CHANGELOG (**BREAKING**), MIGRATION (packagers, before and after).
- No configuration key changes.

## Deviations from the brief

- **Each feature enables both vcard-rs and ical-rs**, where the brief named vcard-rs for CardDAV and ical-rs for CalDAV: the kind layer (src/kind, its three-way merge on vcard-rs and ical-rs) compiles both kinds whenever any mutable-content backend is on, as `gpeople` and `gcal` already pull both. Splitting the kinds per feature would gate the shared DAV adapter's every `DavKind` arm, which serves both from one client.
- The CalDAV integration suites (tests/accounts.rs, tests/drain.rs) and the unit tests resolving a `caldav` table beside a `carddav` one move under their kind's feature, so a single-kind build runs only what it opens.
