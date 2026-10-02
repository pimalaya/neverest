---
cairn: change
id: a-preferred-local-reaches-the-target
status: active
created: 2026-10-02
---

# A `--prefer-local` resolution reaches the target

Self-contained: a session with no prior context can take it from here.

## Why

"A divergence with no common ancestor parks rather than resolving itself" says either resolution is carried to both endpoints by the next run. `--prefer-remote` is. `--prefer-local` is not: the next run exits 0 and the target keeps its own body, so the two endpoints stay divergent after a person ruled, and nothing reports it.

Found 2026-10-02 by tests/endpoints.rs `two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite`, against Radicale (`./tests/radicale.sh`):

- source (`test`) and target (`test2`) both hold `card-1` and `card-2` before the first sync, `TEL:+1` on the source, `TEL:+9` on the target
- the first sync parks both, exit 2, as specified
- `conflict resolve <card-1> --prefer-local`, `conflict resolve <card-2> --prefer-remote`
- the next sync exits 0; `card-2` is `+9` on both, `card-1` is still `+9` on the target, which should be `+1`

The same on io-pimdir 0.5.0 and 0.5.1. Nothing is lost, both bodies surviving, but the run reports success over an account that is not in sync, which the "reconciled with each other" requirement forbids.

## What

Make the run after a `--prefer-local` resolution push the settled body to the target.

Leads, unverified:

- `--prefer-local` keeps the store's body, which the source already holds, so the store may see nothing dirty: the resolution changes the item's state without staging the push the target needs. `--prefer-remote` stages a new body and so pushes.
- The item's divergence flag, which the requirement says outlives the decision, may be read as settled for the target's binding while its revision still names the target's own body as current, so the round finds the target in sync.

Look at how src/cli/conflict.rs applies `--prefer-local` for an item-level divergence, against what `--prefer-remote` stages.

## Not in scope

The ancestor-less parking itself, which works.
