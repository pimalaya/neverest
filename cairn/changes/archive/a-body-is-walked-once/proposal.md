---
cairn: change
id: a-body-is-walked-once
status: landed
created: 2026-10-14
---

# A body is walked once, by the store's write

Follows io-pimdir `ca4c589` (io-pimdir-linked-body-columns), whose log left neverest's own walk of the parts for later.

## Why

Since io-pimdir `ca4c589`, `client::write::apply` walks every mail body entering the store once (`PimdirAttachmentWalk`, read from the blob) and states the attachment mark and the invitation over what the writer sent; a body linked from the store takes the columns of a row holding it (`held_mail_summary`). neverest still walked the parts itself (`AttachmentMark`, fed by `BodyScan` in the sink of every fetched body, and by `stored_item` over a whole stored blob), a value `apply` then overrode: every downloaded body walked twice, and every body served from the store to a second Gmail label read whole from disk for a mark the store already held.

## What

- `kind::mail::AttachmentMark` removed; `Kind::parse_body` and `mail::parse_body` take no mark again, the `Full`-tier summary leaving `attachment` unknown for the write to state.
- `BodyScan` becomes `HeaderScan`: the header block alone (bounded by `HEADER_CAP`), no walk state; `HydrateSink` takes no kind.
- `stored_item` reads a stored blob up to its header block, 8 KiB a read, rather than to its end.

## Cases checked

- **A body missing from the blob store at write time** (io-pimdir logs it and keeps the writer's mark): moot for a streamed body. `HydrateSink::finish` commits the blob (`PimdirBlobWriter::commit`, fsynced and renamed) before the item is handed to the engine, and the collector cannot run while a verb of the process is between its chunks (`InFlight`) or a producer is staging (§5, §8). `stored_item` answers `None` for a blob gone, landing nothing.
- **A mail body written other than through `apply`**: none. neverest has no SQL of its own; syncs, upgrades and mutations go through the coroutine's `WantsWrite` (`PimdirSourceStore::write`), the paired and conflict paths through `write`, all `write::apply`.
- **neverest reading the mark it computed**: nowhere. No report, log, coverage count or comparison reads `attachment`; the driver reads a summary's `size` and `date` alone (relay, phase 2).
- **A body held by rows from before the rule** (written by neverest before `0a4d7f8`, mark `NULL`): `held_mail_summary` finds no row stating a mark and `apply` keeps the writer's, now unknown, where neverest's walk gave the true one. The new row then reads as the rows already holding the body do. Not walked by neverest: it cannot tell a held body from an entering one before the write, and walking every body to cover this case is the duplicate this change removes. The store's to fill if wanted (walk a held body whose rows state no mark), not the writer's.

## Not in scope

- io-pimdir: walking a held body whose rows state no mark.
