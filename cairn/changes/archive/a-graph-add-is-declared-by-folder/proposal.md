---
cairn: change
id: a-graph-add-is-declared-by-folder
status: landed
created: 2026-11-15
---

# A Graph add is declared by folder

## Why

Live testing of 2026-11-15 (ledger line N15 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15, a dead end): on Microsoft Graph, a message queued into a folder other than Drafts (`himalaya -a <pimdir> message add -m <source>/msgraph/<folder>`) was accepted, `neverest drain` applied it, then every `sync` rejected the append ("Graph creates every MIME message as a draft, so it appends to Drafts only") and exited 3, run after run. The source declared one source-wide `mail.message.add: partial`, which a frontend cannot refuse up front; the rejected row's `code` was null; and the item could not be undone, a delete staging a move the source refuses (`item.create or item.delete is disabled`).

## What

- (a) A Graph mail source whose rights allow an add declares `mail.message.add` `none` source-wide ("Drafts only: Graph creates every MIME message as a draft") and `full` on each collection stating the drafts role (`capability::drafts_only`, called by `declare`), so the store's gate refuses an add elsewhere at enqueue.
- (b) The rejection is coded `unsupported`.
- (c) Not done here: see Decisions.

## Decisions

- **`unsupported`, not `capability-missing`.** `unsupported` is in neverest's vocabulary already, relayed from io-pimdir, whose gate answers it for a write a source does not support, the very refusal (a) now makes at enqueue: one write refused gets one code whichever side refuses it. `capability-missing` is himalaya's, for a verb a binary's backend does not serve; neverest has no such verb.
- **The check keeps `partial`.** A check knows no store collection; "Drafts only" is the doctor's summary.
- **(c) is io-pimdir's.** A pending create is bound to its source under a provisional handle (U+0001, pimdir SYNC §2). The gate reads the holders of an item from every binding (io-pimdir 0.13.0, spec/queries/storage/read/load_item_capabilities.sql, the `holders` CTE; src/client/capability.rs `at_item` and `gate_by_kind`, the `Remove`/`Move` arm checking `mail.message.remove` or `.move` against them), the provisional one included, so a never-pushed item's remove or move needs the source's `item.delete`. Excluding provisional bindings there (`substr(b.handle, 1, 1) != char(1)`, as list_unstamped_bindings.sql does) and dropping the pending create with the item is a library change; neverest cannot reach the producer's gate.
