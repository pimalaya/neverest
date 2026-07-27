---
cairn: change
change: sole-local-store
---

# Delta

## ADDED Requirements

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once — content-addressed in the pimdir
blob store, deduped across sides and mailboxes — and Neverest SHALL keep no parallel
local copy in another format. A maildir/m2dir side is a *source being synced* (one
of the two sides, interop with an existing on-disk store), reconciled through the
same pimdir pivot as an IMAP side, not a local cache alongside pimdir; the
file-per-item local store is superseded by the indexed pimdir store, whose portable
interchange profile is pimdir `EXPORT.md`. The store lives per account as
`pimdir.db` plus an `objects/` blob directory.

## MODIFIED Requirements

## REMOVED Requirements
