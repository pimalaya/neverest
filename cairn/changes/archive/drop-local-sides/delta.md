---
cairn: change
change: drop-local-sides
---

# Delta

## ADDED Requirements

### Requirement: Sides are remote backends only
A sync side SHALL be a remote backend (IMAP today; JMAP/Gmail/Graph as their
backends land). Local file backends (m2dir, maildir) SHALL NOT be sync sides — the
pimdir store is the local replica, so a local file store is redundant as a side and
belongs on the import/export path (io-pimdir conversion), which neverest documents
rather than syncing directly. The wizard SHALL produce a one-side (local-sync)
remote account by default.

## MODIFIED Requirements

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once — content-addressed in the pimdir
blob store (under retain), deduped across sides and mailboxes — and Neverest SHALL
keep no parallel local copy in another format. Sync sides are remote backends only;
an existing on-disk store (maildir/m2dir) is brought in through io-pimdir's
conversion tooling, not synced as a side. The store lives per account as
`pimdir.db` plus an `objects/` blob directory.

## REMOVED Requirements
