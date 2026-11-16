---
cairn: tasks
change: a-feature-is-named-after-its-table
---

# Tasks

- [x] Cargo.toml: `carddav` and `caldav` in place of `dav`, both in the defaults
- [x] src: shared DAV code under `any(carddav, caldav)`, each table's open arm under its own feature, `is_built` and `SourceBackendConfig::feature` naming the table
- [x] src/dav/client.rs: `DavKind::Cal` allowed dead in a build without `caldav`
- [x] Tests: `config::tests::each_dav_table_names_its_own_feature`, `account::build_tests::a_carddav_source_left_out_names_carddav` and `a_caldav_source_left_out_names_caldav`, failing on the code before (`dav`)
- [x] .github/workflows/tests.yml clippy sets, README, CONTRIBUTING, config.sample.toml
- [x] CHANGELOG **BREAKING** line, MIGRATION entry for packagers
- [x] Fold into cairn/spec/sync.md, log
- [ ] An agent runs no git write command; the owning session commits
