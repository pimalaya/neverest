---
cairn: log
change: declared-sync-mode
date: 2026-08-27
---

# The account declares its mode

An account's behaviour used to be derived from a coincidence: two sources that
happened to share a `collection.namespace` mirrored each other, and the number of
sources in that namespace decided whether the store kept every body, the bodies
that crossed, or none. Three things followed, and all three were live.

Direction was not expressible. A namespace said *who* paired with whom and never
*which way*, so both sides had to be authoritative and every divergence became a
conflict; permissions did not fill the gap, `item.create = false` constraining
what may be written rather than declaring an authority. "A is right, make B match
it" could not be said.

The mode changed by accident. `StoredBodies::derive` keyed on source count, so
adding a second source to a namespace turned a full offline replica into a store
keeping no bodies at all. Persisting the previous derivation, reporting it every
run and naming what became unreferenced all existed to soften a transition that
should never have been implicit.

And the result was unreadable, which is where this started: every run printed
`message/rfc822 / imap (imap): bodies all` because the configuration could not be
read to find out.

An account now declares `sources`, optionally `targets`, and the flags `one-way`
and `retain`. The mode is the arity plus the flags, so there is nothing to name:
one source and one target sync both ways, `one-way` makes the source overwrite
the target, one source and several targets is one-way only, and several sources
with no target is the offline replica. Every other cell is refused at load,
naming the nearest legal one.

`one-way` is an authority, expressed as io-replica's conflict policy:
`PreferRemote` on the source, `PreferLocal` on the target, and no push back to an
authoritative source. That is what removes the conflict rather than resolving it.
`retain` is the other half, and it is a boolean where the old derivation had three
points: the store either holds bodies or is only the ledger of spines and
checkpoints it has to be in every mode, including a body-less IMAP to IMAP copy.
Whether a crossing is streamed or staged is now an internal choice, so `crossing`,
a store holding only what happened to cross, is not a state anyone can reach.

`collection.namespace` is gone from the schema and refused by name. The hub still
keys collections by `(kind, namespace, name)`, but the namespace is internal and
defaulted to the source name, and a target binds its source's.

The mode triple is stamped in `neverest.json` and compared every run. Turning
`one-way` on is refused, the run that follows being the one that discards what the
previous mode was merging, and the remedy is `sync --accept-mode` rather than an
`init` or `--reset` that would drop the store. A `retain` dropping to false and a
bare arity change are reported and do not block. Gating is on those transitions
and not on configuration change in general: a rotated credential must never cost a
resync. A first run under `one-way` has nothing to compare against, so `init`
states the account's behaviour in words instead.

The per-run store report is gone with the derivation that needed it. `check`
states the mode in plain language, and the persisted value now serves the refusal
rather than a line nobody could read.

Capabilities moved: **sync** (the account schema, the mode and its guard, the
collection key, the reporting, and `--source` narrowing).
