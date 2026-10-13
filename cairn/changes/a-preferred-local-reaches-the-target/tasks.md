---
cairn: tasks
change: a-preferred-local-reaches-the-target
---

# Tasks

- [x] Reproduce with `cargo test --features dav --test endpoints two_endpoints_holding_one_card_under_two_bodies -- --ignored` on `./tests/radicale.sh`.
- [x] Compare what `--prefer-local` and `--prefer-remote` leave in the store for the target's binding.
- [x] Fix, so the next run pushes the settled body to the target: no neverest code, io-pimdir `189db40` parks the divergence on the target's placement with the source's body as its own, so `--prefer-local` stages the push the target needs (an-ancestor-less-divergence-parks-again).
- [x] The test passes, and the other four in tests/endpoints.rs still do.
- [x] CHANGELOG `Fixed`, log entry, status `landed`.
