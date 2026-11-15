---
cairn: change
id: endpoints-reconcile-with-each-other
status: landed
created: 2026-08-30
---

# Two endpoints of one account are reconciled with each other

## Why

An account naming a source and a target is the mirror and the migration, and it is the only shape where two servers have to be brought into agreement rather than a server and a store. Both of the things that only happen there were wrong, and both lose data.

**A card changed on both endpoints lost one of the changes, silently.** Each endpoint's own reconcile is a two-party affair: it compares what it holds against what its server holds, and here neither of them disagrees with its own server, so neither marks anything. What diverges is the pair, and nothing was looking at the pair. Worse, the way a pull records a remote change is to drop the stale body from the shared item and from that source's base together, so once both endpoints have pulled, the store holds the two new bodies under two bindings and no longer holds the one they both came from. The item then reads as clean on both sides, the run reports an empty patch and exits successfully while the two servers hold different cards, and the next run hydrates the shared body from whichever endpoint it hydrates first and pushes it over the other. Against a Radicale with two principals: edit a card to `TEL:+2` on one and `TEL:+3` on the other, and run one says `already in sync`, run two makes both `+2`, and nobody is told that `+3` ever existed.

The crate already holds the answer to this. `Kind::merge` three-way merges a base against two sides, resolves when nobody disagreed and reports the collisions when someone did, and the one-endpoint path already routes a divergence through it and parks what it cannot settle. What was missing is the ancestor, which the pull throws away, and the shape: nothing gave the pair the form a one-endpoint divergence has.

**Two servers that already hold the same card did not bind it.** Identity is settled by the fetch that reads it, and a collection cannot hold one link id twice, so a fetch resolving an identity another placement already claims mints a key of its own for the second copy (pimdir SPEC §9). That rule is right and it is load-bearing: one mailbox legitimately holds one `Message-ID` twice. But the placements it reads as claimants come from a source's projection, and a projection answers a source with the items it holds *plus* the copies a sibling source holds and it does not, so that the merge can derive the append. The second endpoint therefore read the first endpoint's card as a claim on the identity, and minted `dup:card-1#card-1.vcf` for its own. Two items, one per endpoint, then each refused by the other server on `no-uid-conflict`. Only an account whose card starts on one side and propagates ever bound correctly, which is exactly not the mirror case.

## What

- Read the shared body of every item before a reconcile round pulls, because pulling is what loses it, and only where the pair reconciles with each other at all.
- After the round, find the items both endpoints rewrote: the item holds no body, neither binding holds a base body any more, and one was there before the round. One endpoint alone leaves the other's base body in place, which is what tells the two cases apart.
- Give the pair the shape a one-endpoint divergence has: hydrate the source's body as the shared one, and record the target's as the divergence against it, with the ancestor as its base and the revision its own pull observed. The existing conflict resolution then fetches the target's body, merges the three and stages what settles.
- Park what no merge settles, report it once, and leave both servers holding what they hold. The source is the merge's left side, `ours`, which is what decides the shared body; it decides nothing about a collision, which goes to a person through the conflict commands.
- Read the store through a view of what a source actually holds where identity is settled, so a copy the hub is offering a source is not read as that source holding the identity.
