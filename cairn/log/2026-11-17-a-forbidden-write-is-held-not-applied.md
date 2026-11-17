---
cairn: log
change: a-forbidden-write-is-held-not-applied
date: 2026-11-17
---

# A forbidden write is held, not applied

Finding NEV-3 of the CLI family analysis of 2026-11-15: the itemized plan was blind to rights, so with `item.create`, `item.update` and `item.delete` false on a target every run listed a delete, an update and a copy as made ("synchronized: 3 hunks", exit 0) while the target received no write.

## What landed

- src/sync/driver/itemize.rs: `itemize` and `itemize_single` drop a hunk the side's rights forbid (`forbidden_action`) and count it under the new report row `forbidden` (`ForbiddenWrites`: side, collection, action, count), with no warning and no exit code.
- Fake CardDAV: `remove`, and a delta naming removed cards with a 404 row.
- Test tests/pair.rs `writes_a_target_s_rights_forbid_are_reported_held_never_applied`, failing with the hunks listed without the filter.

## Capabilities moved

- [sync](../spec/sync.md): *A source may be denied item updates* amended.
