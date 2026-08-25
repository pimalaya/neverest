---
cairn: log
change: duplicate-link-id-freeze
date: 2026-08-25
---

# An identity a collection holds twice is reported, and every write is

The third repo of the cross-repo change, after io-replica (the invariant, the detection, the rules) and io-pimdir (the persistence, and the write that used to destroy the evidence). Those two make the engine derive nothing for an identity a source holds under two handles; what was left here is the part only this crate can do: telling the user, in the language of mailboxes and UIDs rather than of placements, and proving the chain end to end against real servers.

## What landed

- **`SyncReport::ambiguous`**, rendered in the text report's warnings section and under `ambiguous` in `--json`, naming the side, the collection and every handle the side holds the identity under, the bound one first. Read off the projection beside the conflict pass (`ambiguity` in `src/offline/driver.rs`, called from both `itemize` and `itemize_single`), so the one-side and two-side runs both report it. Re-reported on every run for free: the freeze is persisted on the binding, and nothing here could rediscover it, since the second copy appears in exactly one enumeration.

  The wording is the load-bearing part. RFC 5322 §3.6.4 binds the *generator* of a `Message-ID`, a copy legitimately carries the identifier of the message it copies, and duplicates commonly arrive from a migration, which is this tool's own use case. So the report says neverest cannot tell the copies apart, never that the mailbox is invalid, and it names no repair: which copy to keep is the user's, with their own client.

- **`placement_hunks` derives nothing for `Ambiguous`**, on every axis, which is what the engine already decided; the status enum forced the arm.

- **A relayed copy is itemized.** The defect step 3 of the reproduction exposed was this crate's own, and wider than the reproduction: a two-IMAP account relays by default, and `relay_copies` streamed the body straight from one server to the other and returned a count. A relay never reaches the projection the report is built from, so **every** relayed append was invisible and a run that appended messages could print `already in sync`. Each relay now pushes the `Copy` hunk the hydrating path reports from the projection, under the same link id, so the two paths report identically and `already in sync` again means the run wrote nothing.

## Adapting to the engine and the store

Neither upstream is published, so `[patch.crates-io]` points both at the sibling working trees; the requirements in `[dependencies]` stay honest and the block is one delete once they release. Taking the freeze meant taking what the audits beside it carried: `ReplicaStorage::load` takes a `ReplicaLoadScope`, `ReplicaYield::WantsLoad` is a struct variant, `ReplicaPlacement` gained `ambiguous_handles`, and `ReplicaChange::Remove` gained a `link_id`.

That last one is recorded rather than used: it is the identity a relocation would deliver, which a consumer able to ask "does `to` already hold it?" uses to demote a move to a plain delete. No backend seam here answers that short of enumerating the destination, so both halves of a move can still deliver. The consequence is now a frozen, reported duplicate rather than a silent mispairing, which is a fair place to leave it until a seam exists.

io-pimdir's handle also split by what an operation needs (`sourceless-store-handle`): `PimdirStore::open(dir)` names no source and `for_source` yields the sync seam, so the driver's per-side handles are `PimdirSourceStore` and the source-less reads reach through its `Deref`.

## Verification

- 70 unit tests green, `cargo clippy --all-features --all-targets` clean, `cargo fmt`.
- `tests/duplicates.rs`, **run live** against the two Stalwart instances (`tests/stalwart2.sh`), replaying the whole reproduction with a per-run identity: one copy on A and two on B freezes the identity with no hunk derived and a warning naming both UIDs; expunging one copy on B pushes no delete and leaves A's copy standing; emptying B's checkpoint (which the IMAP backend reads as absent, so the next enumeration is full) re-appends nothing to A.
- `tests/relay.rs` still green live, and a relayed copy verified to appear in the `--json` report as a `copy` hunk.
- Unit level: the freeze survives the store round trip, projects `Ambiguous`, derives no hunk and renders in both formats with its coordinates; the relay plan carries the identity its hunk is reported under.

## Known blocker

`a_rekey_carries_state_by_link_id_and_bumps_the_generation_once` fails against the current io-pimdir working tree, and not because of anything here. A rekey emits `DropPlacement { reason: Superseded }` for the whole old spine plus an upsert under each new handle, and states that batch is order-insensitive. io-pimdir's `save_bindings_diff` compares the hub before and after the whole batch, which collapses that into "same source, different handle", and the freeze floor it grew then keeps the old handle and records the new one as ambiguous. So a UIDVALIDITY bump freezes every item of the collection instead of carrying it over. The fix belongs upstream: the diff has to know which handles the batch superseded.

Capabilities moved: `sync`.
