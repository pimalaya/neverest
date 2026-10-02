---
cairn: tasks
change: google-sources
---

- [x] `GoogleConfig`, the `gpeople` and `gcal` backend keys and their sugar
- [x] `src/gpeople/client.rs`: listing, sync-token enumeration, reads, create with UID check, update, delete
- [x] `src/gcal/client.rs`: calendars, series grouping, joined revisions, sync-token enumeration, reads, import or insert, update, delete
- [x] `Client` and `SourceAccountBackend` arms, the kind gates widened to the new features
- [x] Tests: config, People collection, Calendar revisions
- [x] Spec, README, CHANGELOG, config.sample.toml
- [ ] Wizard entries for both backends
- [ ] Push a locally modified instance of a series
- [ ] End-to-end run against a throwaway Google account
