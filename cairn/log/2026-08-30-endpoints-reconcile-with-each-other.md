---
cairn: log
change: endpoints-reconcile-with-each-other
landed: 2026-08-30
---

# Two endpoints of one account are reconciled with each other

An account naming a source and a target is the mirror and the migration, and both of the things that only happen there were losing data. A card changed on both endpoints lost one of the changes without a word, and two servers that already held the same card never bound it.

## A card changed on both endpoints

Each endpoint's own reconcile compares what the store holds against what that endpoint's server holds. When both servers move, neither of them disagrees with its own server, so neither marks anything: what diverges is the pair. And the way a pull records a remote content change is `pull_content` dropping the stale body from the placement and from its base together, which the hub then absorbs as the shared body going away. Once both endpoints have pulled, the shared item holds no body, both bindings hold no base body, and the one body they both came from is referenced by nothing. The item projects clean on both sides, so the run reported an empty patch and exited 0 while the two servers held different cards; the retain hydration then filled the shared body from whichever endpoint it reached first, the other side read dirty against it, and the next run pushed one body over the other.

Against a Radicale with two principals, `TEL:+2` on one side and `TEL:+3` on the other: run one said `already in sync`, run two made both `+2`, and nothing ever named `+3`.

**[reconcile_pass](../../src/offline/driver.rs) now reads the shared body of every item before it pulls**, `shared_bodies`, and only where the pair reconciles with each other at all (`parks_divergences`: both endpoints authoritative, not a dry run). That is the merge's common ancestor, and reading it afterwards is impossible.

**`diverged_items` finds what both endpoints rewrote**, from the store alone: the shared item holds no body, both bindings hold a base with no body, and the ancestor snapshot had one. One endpoint alone leaves the other's base body in place, which is exactly what separates "one side moved" from "both did".

**`park_divergences` gives the pair the shape a one-endpoint divergence already has.** It hydrates the source's handle to `Full`, so the source's body becomes the shared one and is therefore the merge's left side, `ours`. It then writes the target's placement as `Conflict`, carrying the ancestor as its base object and the revision the target's own pull observed as both its base revision and its `conflict_revision`, with the body still wanted. `resolve_conflicts` takes it from there unchanged: it fetches the target's body into `conflict_object` and `merge_conflicts` runs `Kind::merge(base, local, remote)` with the source's body local. A `Merged::Body` is staged through the queue as an ordinary edit and both endpoints converge on it in the same run; a `Merged::Collided` stays parked, is counted among the outstanding conflicts, exits 2, and is listed by `neverest conflict list`.

Nothing was added to the resolution path, the report path or the conflict commands: the divergence is given the shape they already read.

## Two servers already holding one card

Identity is settled by the fetch that reads it. A collection cannot hold one link id twice, so io-replica's upgrade mints `dup:<hint>#<handle>` for a fetch resolving an identity another placement already claims (pimdir SPEC §9). The rule is load-bearing, one mailbox legitimately holding one `Message-ID` twice, and the claimants it reads are whatever the storage answered with. A pimdir source store answers with the source's projection, which carries the copies a *sibling* source holds and this one does not, so that the merge can derive the append. The second endpoint therefore read the first endpoint's card as a claim, and minted `dup:card-1#card-1.vcf` for its own. Two items, one per endpoint, each then refused by the other server on `no-uid-conflict`.

**[HeldStore](../../src/offline/storage.rs)** wraps a source's store and drops from every load the placements whose link id that source is not bound to, reading the holders out of the hub once at open. An unlinked placement stays: it is the freshly probed row the upgrade was called for and it claims nothing yet. `upgrade_probed` is the one caller, being the one place identity is settled; the hydration and conflict upgrades only ever name already-linked placements.

## Tests

[tests/endpoints.rs](../../tests/endpoints.rs), two live runs against the two Radicale principals, each owning an address book of its own so they run side by side. One drives a disjoint edit and a same-field collision on the same run, checking the first merges on both endpoints, the second parks with exit 2, and neither body is overwritten by a rerun. The other seeds one card on both servers before the first sync and checks the store holds it once, under its own key, with nothing refused.

`a_copy_on_offer_is_not_read_as_a_holding_of_the_side_it_is_offered_to` covers the projection reading without a server.

## Capabilities moved

- sync: two endpoints reconcile with each other, merging what nobody disagreed about and parking the rest
- sync: one identity is one item across an account's endpoints; the duplicate-minting rule is one source's collection only
