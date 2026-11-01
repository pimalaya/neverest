---
cairn: log
change: a-moved-uidvalidity-rekeys-first
date: 2026-10-30
---

# A moved UIDVALIDITY rekeys first

Release blocker B1 of the pimdir stable-release review: a renumbered IMAP mailbox was listed under its new UIDs before the driver rekeyed it, after the run.

## What landed

- src/client.rs: `Listed::Rekey`. src/offline/remote.rs: `PimRemote::enumerate` records it and fails the enumeration; `Bound` takes the open round's checkpoint while no round has closed.
- src/imap/backend.rs: `renumbered` compares the `UIDVALIDITY` a `SELECT` answers with the store's; every path names a rekey on a mismatch (QRESYNC delta, delta without QRESYNC, delta head, band head, a round's first and resumed pages). A cursor older than a rekey the store already made still restarts the round.
- src/offline/driver.rs: `sync_side_rebuilding` runs `PimdirRekey` when a sync fails `Renumbered`, then syncs again; the post-sync epoch comparison and `Client::handle_space_epoch` are gone. A failed rekey leaves the checkpoint, so the next run rekeys.
- src/imap/fake.rs: a scripted IMAP server for the connector tests.
- Tests: `a_moved_uidvalidity_names_a_rekey_on_every_listing`, `a_rekeyed_store_lists_the_new_uids`, `a_renumbered_mailbox_is_rekeyed_before_it_is_listed`.

## Evidence

On the old code the connector answered `[("qresync delta", "page"), ("plain delta", "page"), ("delta head", "head rejected"), ("band head", "page"), ("first page", "page"), ("resumed page", "cursor rejected")]`, and a sync over a renumbered mailbox lost the flag staged on its message (`left: [("7", false)]`).

## Capabilities moved

- sync: an IMAP handle-space change rebuilds the collection (modified: the listing fails and the rekey runs first); IMAP enumeration is incremental (modified: a moved UIDVALIDITY names a rekey).
