---
cairn: change
id: declared-sync-mode
status: landed
created: 2026-08-27
---

# The account declares its mode, instead of the mode being inferred

## Why

An account's behaviour is currently derived from a coincidence: two sources that
happen to share a `collection.namespace` mirror each other, and the number of
sources in that namespace decides whether the store keeps every body, the bodies
that crossed, or none. Nothing is declared. The user writes endpoints and gets a
mode.

Three things follow from that, and all three are visible today.

**Direction is not expressible.** A namespace says *who* pairs with whom and
never *which way*, so the engine has no choice but to treat both sides as
authoritative and three-way merge them. Permissions do not fill the gap:
`item.create = false` constrains what may be written, it does not declare an
authority, so both sides are still diffed and a conflict is still reachable. The
one thing a user migrating from one provider to another actually wants to say,
"A is right, make B match", cannot be said.

**The mode can change by accident.** `StoredBodies::derive` keys on source
count, so adding a second source to a namespace silently turns a full offline
replica into a store that keeps no bodies at all. The whole apparatus of
persisting the previous derivation, reporting it every run and naming what became
unreferenced exists to soften a transition that should not have been implicit in
the first place.

**Nobody can read the result.** What the store keeps is reported on every run as
`message/rfc822 / imap (imap): bodies all`, because the configuration cannot be
read to find out. A line that exists only because the config is unreadable is a
symptom, not a feature.

The three states also leak an artefact. `crossing`, the state a pairing that
cannot stream falls into, is a store that is neither a replica nor a pure relay:
it holds bodies for the items that happened to cross and nothing else. It is not
a mode anyone asked for. It is what is left over when two endpoints must exchange
bodies and the pairing is not IMAP to IMAP.

## What

An account declares `sources`, optionally `targets`, and two flags. The mode is
the arity plus the flags, so there is no mode name to invent and no namespace to
learn:

| sources | targets | `one-way` | behaviour |
|---|---|---|---|
| 1 | 1 | false | two-way mirror between the two remotes |
| 1 | 1 | true | source overwrites target |
| 1 | N | true | source overwrites each target (false is refused) |
| N | 0 | false | each source merges two-way with the local store |
| N | 0 | true | sources overwrite the local store, local edits discarded |

- `one-way` declares authority: the `sources` side wins and the other side's
  changes are discarded. It does not mean the other side goes unread, since it is
  still enumerated every run or every item would be re-pushed. Overwritten, not
  merged.
- `retain` declares whether the local store is also a readable replica, separately
  from who the endpoints are. pimdir is never nameable as an endpoint: it is
  always the ledger holding the spine and the checkpoints, required even for a
  body-less IMAP to IMAP copy, and `retain` decides whether it additionally holds
  bodies.
- `targets` is a named map like `sources`, so every endpoint carries a stable
  pimdir source id. A positional list would reassign every binding when the list
  is reordered, which is why `left` and `right` were removed and is not worth
  reintroducing under another name.
- `collection.namespace` is gone from the configuration surface. The hub key keeps
  a namespace internally, defaulted to the source name, so collections of
  different kinds still cannot collide.

Streaming a crossing rather than staging it becomes an internal optimisation of
`one-way`, chosen when the pairing allows it. `crossing` stops being a state a
user can be in.

`NeverestState` already records what a store was created under and refuses it when
that no longer matches. It gains the mode triple, compared on every run, gating
the transitions that destroy data rather than configuration change in general: a
rotated password must never force a resync.

## Not in scope

**No third flag for partial retention.** `retain` is a boolean. The store either
holds every body for what it syncs or holds none, and the leftover middle state is
what this change exists to remove.

**No first-run guard.** A first run with `one-way = true` against a non-empty
target is destructive and has no prior state to compare against, so nothing
detects it. It stays ungated: the user wrote that configuration deliberately, and
rsync does not ask either. `init` states the account's behaviour in words and
`sync --dry-run` covers the rest.
