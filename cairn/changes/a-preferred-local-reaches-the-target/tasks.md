---
cairn: tasks
change: a-preferred-local-reaches-the-target
---

# Tasks

- [ ] Reproduce with `cargo test --features dav --test endpoints two_endpoints_holding_one_card_under_two_bodies -- --ignored` on `./tests/radicale.sh`.
- [ ] Compare what `--prefer-local` and `--prefer-remote` leave in the store for the target's binding.
- [ ] Fix, so the next run pushes the settled body to the target.
- [ ] The test passes, and the other three in tests/endpoints.rs still do.
- [ ] CHANGELOG `Fixed`, log entry, status `landed`.
