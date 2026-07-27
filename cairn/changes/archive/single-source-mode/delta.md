---
cairn: change
change: single-source-mode
---

# Delta

## ADDED Requirements

### Requirement: Side count selects the sync mode
An account SHALL configure one or two sides (`left`/`right`, each optional; at
least one required). **One** configured side is a *local sync*: that remote is
reconciled against the retained pimdir store, which is the local replica an app
reads and edits. **Two** configured sides is the remote-to-remote sync through the
store. The store is otherwise implicit (per-account state dir) and customised only
by an account-root `store` config (`root` override), never as a side.

### Requirement: A local sync retains every body
A one-side sync SHALL hydrate every synced item to `Full` (fetch its body into the
store), because the store is the app's offline copy — distinct from the two-source
path, which hydrates only bodies about to cross. It SHALL pull before pushing so an
edit the app staged locally stays pending and is reported (and pushed) rather than
swallowed, and it SHALL open the store as the one side's source so an app writing
as that same source stages edits the sync pushes.

## MODIFIED Requirements

### Requirement: Two sources over one store
When two sides are configured, they SHALL be two source handles (`"left"` /
`"right"`) of one pimdir store, the mailbox name as the bare collection id; the
shared database is the cross-side hub and cross-side propagation of messages, flags
and deletions falls out of the hub's project/absorb. A single side instead syncs
that one remote against the store as the sole local replica.

## REMOVED Requirements
