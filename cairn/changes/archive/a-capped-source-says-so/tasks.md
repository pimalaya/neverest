---
cairn: tasks
change: a-capped-source-says-so
---

# Tasks

- [x] `collection.capped` in `CollectionSourceConfig` (src/config.rs), false by default
- [x] `SourceCtx::capped`, `PimRemote::with_capped`, `capped` on every page of a round (src/offline/driver.rs, src/offline/remote.rs)
- [x] `unlisted` in the report, its text block and its JSON (src/sync/report.rs)
- [x] `a_capped_source_keeps_what_its_listing_leaves_out`, seen failing with the seam stating `capped: false`
- [x] config.sample.toml, README, CHANGELOG, spec, log
- [ ] A rekey of a capped collection: pimdir SYNC §8 states no rule for a cap, and io-pimdir 0.11 reads none (the standard's, then the engine's)
- [ ] A server test over Gmail's IMAP with the folder limit set (the owner)
