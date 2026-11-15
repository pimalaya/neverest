---
cairn: tasks
change: neverest-follows-io-webdav-0-9-1
---

# Tasks

- [x] `cargo update -p io-webdav` (0.9.0 to 0.9.1, nothing else moved); Cargo.toml requires "0.9.1"
- [x] Read where neverest builds a DAV path or hands io-webdav an id: none built, none taken from the command line (see the proposal)
- [x] Test `an_id_climbing_out_of_its_collection_sends_nothing` (src/dav/client.rs): collection keys `..`, `/`, `""`, `a/../..` and member ids `..`, `.`, `""` send nothing; it fails on io-webdav 0.9.0, the create answered 201
- [x] Check the exact bodies against the store: no re-download, no change seen (see the proposal)
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
