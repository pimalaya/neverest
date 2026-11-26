---
cairn: change
id: the-docs-cite-storage-and-sync
status: landed
created: 2026-10-30
---

# The docs cite STORAGE and SYNC

Landed (citations and stale statements fixed by 2026-11-19); closed on 2026-11-26, its CHANGELOG step moot since the 1.0.0 section was rewritten (log 2026-11-25-docs-aligned-for-the-major). Log 2026-11-26-the-docs-cite-storage-and-sync.

## Why

Pimdir stable-release review (2026-10-30), stale docs. The pimdir spec is split into STORAGE.md, SYNC.md and SEARCH.md, yet code comments and this spec still cite "pimdir SPEC §n" (src/offline/driver.rs, src/offline/remote.rs, src/cli/conflict.rs, cairn/spec/sync.md), and some statements no longer hold: src/offline/driver.rs:15 says collection deletion is not propagated, cairn/spec/sync.md names `PimdirDeletePolicy::Keep`, a type gone from io-pimdir, src/kind/merge.rs:7 says the merge rides on `dav` alone. The CHANGELOG's [Unreleased] cites about eighteen intermediate revisions and pimdir draft numbers where it should be a net diff.

## What

- Every "pimdir SPEC §n" names its document and section (STORAGE, SYNC, SEARCH).
- The stale statements fixed.
- CHANGELOG [Unreleased] rewritten as a net diff: final dependency versions instead of revisions, one line per dependency, an upgrade note (0.3.0 stores migrate in place, older draft stores need `sync --reset --force`).
