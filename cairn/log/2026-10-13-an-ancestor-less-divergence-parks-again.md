---
cairn: log
change: an-ancestor-less-divergence-parks-again
date: 2026-10-13
---

# An ancestor-less divergence parks again

io-pimdir `189db40` (pushed, released after 0.8.0) carries the fix of its change `a-landed-create-holding-another-body-conflicts`: a page landing a pending create over a mutable arrival holding another body is the both-changed case of pimdir SYNC §6, the landed placement's base holding no body, so the conflict policy decides. Under `Manual` the target's placement parks with the source's body as its own and its own body as the divergence.

## What landed

- Cargo.toml: io-pimdir `0.8`, patched to `189db40` through `[patch.crates-io]`, with the other Pimalaya libraries (log 2026-10-13-pimalaya-libs-from-pushed-revisions).
- tests/endpoints.rs `two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite` passes on Radicale (`./tests/radicale.sh`), and fails on `be03184` (the first sync exits on the wrong code); the four other tests of the file pass too.

## Capabilities moved

None: sync's "A divergence with no common ancestor parks rather than resolving itself" already required it.
