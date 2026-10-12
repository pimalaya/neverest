---
cairn: tasks
change: an-ancestor-less-divergence-parks-again
---

# Tasks

- [x] Find the cause: io-pimdir's page landing pushing a staged body over an arrival it never agreed with
- [x] Fix it in io-pimdir with a regression test (tests/engine/hub.rs)
- [x] tests/endpoints.rs green on Radicale against `be03184` with the fix applied, and the other Radicale and Stalwart suites
- [ ] Commit the io-pimdir fix and pin its rev in Cargo.toml
- [ ] Rerun tests/endpoints.rs on the pinned rev; land a-preferred-local-reaches-the-target with it
- [ ] CHANGELOG `Fixed`, log entry, status `landed`
