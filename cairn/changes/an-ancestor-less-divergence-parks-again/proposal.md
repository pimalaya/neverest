---
cairn: change
id: an-ancestor-less-divergence-parks-again
status: landed
created: 2026-10-11
---

# An ancestor-less divergence parks again

## Why

Found by the 2026-10-11 audit baseline: tests/endpoints.rs `two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite` fails deterministically on io-pimdir `be03184`. Two endpoints holding one `UID` under two bodies before the first sync end with the source's body on both: the run reports one `update` hunk on the target, exit 0, and the target's body is gone. "A divergence with no common ancestor parks rather than resolving itself" is broken, data loss.

The cause is in io-pimdir, since `35a1c3f` moved naming into the page: the hub projects the source's item onto the target as a pending create, and the target's listing, carrying the same `UID`, lands that create (SYNC §6). Landing took the arrival's body as the base and kept the staged body as the placement's, so the source's body read as a local edit and was pushed over the target's. The hub never saw a cross-source divergence, so `park_hub_conflicts` had nothing to park.

## What

- io-pimdir (uncommitted in ../io-pimdir, change `a-landed-create-holding-another-body-conflicts`): a mutable arrival holding another body than the staged create is the both-changed case SYNC §6 names, so the landed placement's base holds no body and the policy decides. Under `Manual` the target's placement parks with the source's body as its own and its own body as the divergence, the base holding none, which is the shape this account's parking already has, so the merge leaves it alone and both `conflict resolve` directions settle it.
- Here: pin io-pimdir to the commit carrying it. No neverest code change: with that engine the whole test passes on Radicale, `--prefer-local` and `--prefer-remote` both reaching both endpoints, which is also what a-preferred-local-reaches-the-target asks.
