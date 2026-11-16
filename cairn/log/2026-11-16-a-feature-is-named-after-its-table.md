---
cairn: log
change: a-feature-is-named-after-its-table
date: 2026-11-16
---

# A feature is named after its table

Line L21 of the CLI alignment round of 2026-11-15, owner ruling D33 of 2026-11-16.

## What landed

- Cargo.toml: the `dav` feature split into `carddav` and `caldav`, both in the default set, each enabling io-webdav, io-http, vcard-rs and ical-rs (the kind layer builds both kinds whenever a mutable-content backend is on, as under `gpeople` and `gcal`). No `dav` alias.
- Shared DAV code under `cfg(any(feature = "carddav", feature = "caldav"))`; opening a `carddav` source under `carddav`, a `caldav` source under `caldav` (src/account.rs). `DavKind::Cal` is allowed dead in a build without `caldav` (src/dav/client.rs).
- `config::is_built` and `SourceBackendConfig::feature` name each DAV table's own feature, so the `backend-not-built` refusal reads `--features carddav` or `--features caldav`.
- Tests: `config::tests::each_dav_table_names_its_own_feature` and `account::build_tests::a_carddav_source_left_out_names_carddav`, `a_caldav_source_left_out_names_caldav`, failing on the code before (it named `dav`). tests/accounts.rs and tests/drain.rs, CalDAV suites, build under `caldav`.
- CI clippy sets `rustls-ring,carddav` and `rustls-ring,caldav` in place of `rustls-ring,dav`; README, CONTRIBUTING (its stale line holding `carddav` out of the defaults dropped), config.sample.toml, CHANGELOG **BREAKING**, MIGRATION for packagers.

## Capabilities moved

- [sync](../spec/sync.md): *Every remote backend is a cargo feature* names `carddav` and `caldav`, one feature a table, and gains the scenario *A CalDAV source in a CardDAV-only build*.
