---
cairn: log
change: the-report-names-settle
date: 2026-11-17
---

# The report names settle

The owner's rulings of 2026-11-17 on the names neverest v1.0.0 freezes: the `refused` and `refusals` pair and the schema review's C11, C3 and A4.

## What landed

- The report's `refused` (creates a side refused because it already holds the item's `UID`) is `duplicates`, so it no longer sits one suffix from `refusals` (changes a source refused as they stand).
- The events `opened`, `listed` and `refused` are `endpoint-opened`, `endpoint-listed` and `change-refused`, prefixed by their subject as `account-started` and `collection-started` are (C11).
- A coverage row's `at` is `closedAt`, read as `queuedAt` and `appliedAt` are (C3).
- A `rejected` row's `action` is `kind`, as every other row names a write's kind (A4); so is a `forbidden` row's, added the same day.
- The owner kept `failures`, `forbidden` and the `sync` hunk kind as named.

## Spec

sync.md: the events paragraph, the coverage and `forbidden` rows, the item error rows, the `rejected` creates paragraph, the empty-report scenario.
