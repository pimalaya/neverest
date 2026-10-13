---
cairn: log
change: a-preferred-local-reaches-the-target
date: 2026-10-13
---

# A `--prefer-local` resolution reaches the target

Settled by the same io-pimdir fix as an-ancestor-less-divergence-parks-again, no neverest code: with `189db40` the divergence parks on the target's placement with the source's body as its own and the target's as the divergence, so `conflict resolve --prefer-local` restates a body the target does not hold and the next run pushes it, as `--prefer-remote` already did.

## What landed

- tests/endpoints.rs `two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite`, which resolves one card each way, passes on Radicale: both endpoints converge on the body each resolution chose. Reported as https://github.com/pimalaya/neverest/issues/31.

## Capabilities moved

None: the scenario "A parked divergence settles either way round" already required it.
