---
cairn: tasks
change: duplicate-link-id-mints-an-item
---

# Tasks

- [x] Bump io-replica (the mint, no ambiguity surface), io-pimdir (no column) and io-webdav (the named refusal). Taken through `[patch.crates-io]` on the sibling working trees while none of the three is published.
- [ ] Put the io-pimdir, io-replica and io-webdav `[patch.crates-io]` entries back to git or crates.io once those crates are released. They point at `path = "../<crate>"` so the local tree is testable end to end, which is deliberate and temporary.
- [x] src/dav/client.rs: `resource_id` carries the minted key's distinguishing part into the resource name, and never re-derives a name from a body whose identity is already taken. `sanitize` keeps its job of making one path segment.
- [x] src/offline/remote.rs: `append` checks the assigned handle against the handles this source already binds in the collection, and returns a rejection instead of a binding when it collides.
- [x] src/offline/remote.rs: a rejected push carrying the no-uid-conflict error is reported as a duplicate `UID` refusal, naming the `UID`, not as a bare status.
- [x] src/sync/report.rs, src/sync/hunk.rs, src/offline/driver.rs: the ambiguity list, its itemisation (`ambiguity`, the `ambiguous` field, the `--json` key) and the `Ambiguous` placement branch go. Nothing in src/sync/hunk.rs existed for it.
- [x] src/kind/mod.rs: one place splits a link id into its hint and its mint (`Kind::split_link_id`), and both write paths take their identity from it. It also replaced the hand-rolled `mid:` parse in the relay plan, which had answered `None` for every item.
- [x] Check `itemize_fetches` reports nothing once the twin has a row, on a collection whose server has no `sync-collection` (the reported case), not only on an incremental one.
- [x] Tests: two resources under one `UID` mirror as two items with two bodies; a re-run reports nothing; an append pair lands under two names; a create answered with a bound handle is rejected; a `no-uid-conflict` refusal is reported with its `UID`; the existing ambiguity fixtures are rewritten to the new outcome rather than deleted.
- [x] `cargo test`, `cargo clippy --all-targets`, `cargo fmt`.
- [x] CHANGELOG: the ambiguity bullet was unreleased, so it is rewritten to the refusal it became rather than paired with a `### Removed`, and the DAV bullet states how a duplicated `UID` now syncs and is named.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; append `cairn/log/2026-08-28-duplicate-link-id-mints-an-item.md` naming the superseded change and the Posteo evidence; mark `landed`, left beside `duplicate-link-id-freeze` rather than archived, as that one was.
