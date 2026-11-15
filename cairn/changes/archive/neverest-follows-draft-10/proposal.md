---
cairn: change
id: neverest-follows-draft-10
status: landed
created: 2026-11-13
---

# neverest follows draft-10

## Why

io-pimdir 0.11.0 (crates.io, the tree of 49e1fd5) follows pimdir ae8b231: storage-draft-10, sync-draft-10 and search-draft-10. A store an earlier draft wrote is refused by every profile, never migrated, so every neverest store is created again. Beyond the compile breaks, the drafts move what a connector and an owner owe: an accepted remove says whether it relocated its member, an add accepted with no handle is a delivered create, an intent whose outcome is unknown is parked under one reserved prefix, an owner keeps its own files under the store's local/, a payload of an unknown version stays pending, and a run reports the creates it leaves waiting and the members it named apart from their create. The audit of 2026-11-12 (pimdir cairn/changes/audit-2026-11-12, lines F14, F16, Y2, Y7) names neverest for each.

## What

- Cargo.toml: `io-pimdir = "0.11"` from crates.io, the git patch dropped.
- The build: `relocated` on every push outcome, `base` on a `set-flags`, `if_match` and `resolves` on a `remove`, an enqueue answering `PimdirEnqueued`, `enqueue_with_body` for the checked one, the `Unconfirmed` and `Reset` statuses, a receipt's optional collection.
- The push contract (Y2): an accepted relocation states `relocated`; a remove whose target already holds the identity deletes instead, on its precondition; an IMAP append the server does not name is accepted with no handle.
- The owner's files (F14): neverest.json, sync.lock and the `submitting` mark move under the store's local/, `0700`, each file `0600`. A sidecar found beside the database is an earlier neverest's.
- The unconfirmed intent (F16): a send that may have been delivered is parked through `unconfirm_action`, under `unconfirmed: `; the report member `uncertain` becomes `unconfirmed`.
- The report (Y7): `waiting`, the creates left waiting with their reason, and `disagreed`, the members named apart from their create; the code `lock-replaced`.
- A create naming no member is owed no body fetch, and a reset counts a delivered create as staged work.
- An intent stating an integer `v` neverest does not know stays pending and untouched.

## Not in this change

- The Graph pass still states gone what it no longer lists (Y1): see the log, the standard leaving the member a band page listed and the server dropped before the pass with no conformant way out.
- D2, Y4, P5, N1, N2, an `Abandon` verb.
