---
cairn: delta
change: the-releases-of-2026-11-19
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A run reclaims retained items on a schedule
The sweep SHALL purge every expired retained item of the account in one transaction (`purge_items`), never one transaction an item; scenario *One write a sweep* added. The opening line of sync.md names io-pimdir 0.18 (pimdir e188e31), still draft-14.

## REMOVED Requirements

None.
