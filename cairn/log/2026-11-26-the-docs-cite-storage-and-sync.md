---
cairn: log
change: the-docs-cite-storage-and-sync
landed: 2026-11-26
---

# The docs cite STORAGE and SYNC

Logged on closing the change (2026-11-26); its edits landed between 2026-10-30 and 2026-11-19.

## What landed

- Every "pimdir SPEC §n" in src/, tests/ and cairn/spec/ names its document and section (STORAGE, SYNC, SEARCH); logs keep their old citations as history.
- Stale statements fixed: the driver's module header on collection deletion, the spec's `PimdirDeletePolicy::Keep` (a type gone from io-pimdir), the merge module's header.
- The CHANGELOG net-diff step is moot: the 1.0.0 section was rewritten whole by docs-aligned-for-the-major (2026-11-25).

## Spec

[sync](../spec/sync.md): citations and wording only, no requirement moved.
