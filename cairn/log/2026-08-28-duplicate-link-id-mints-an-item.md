---
cairn: log
change: duplicate-link-id-mints-an-item
date: 2026-08-28
---

# The phantom fetch stops by itself, and the write side stops losing the copy

The fifth repo of the cross-repo change, after pimdir (the rule), io-replica (the mint), io-pimdir (the column goes) and io-webdav (the refusal is named). It **supersedes `duplicate-link-id-freeze`**, landed here three days ago, on the evidence that change was written to fix.

## What the user saw

Every `neverest sync` of the Posteo account printed the same four `fetch item` lines for `caldav/default`, run after run, naming resources the store never came to hold. Reading the store told the whole story. That calendar holds 454 items and four `UID`s under two hrefs each, `<uid>%40google.com.ics` and `<uid>%2540google.com.ics`, two different resource names both written by Thunderbird; three pairs differ only in `DTSTAMP` and `LAST-MODIFIED`, and the fourth is two genuinely different meetings sharing one `UID`, so no rule about picking a survivor would have been safe.

The engine bound one href per identity and froze the second, so the twin got no row. The pull plan reads the side rather than the projection, on purpose, and a row-less handle there is a body still to fetch, which is exactly what it reported. Then, because Posteo advertises no `sync-collection` for that collection, the fallback listing enumerates it in full on every run: the twin came back, was downloaded whole to resolve its identity (a calendar resource has no cheap `Meta` tier), lost the claim again, and left its body unreferenced. Four downloads and four orphan blobs per sync, indefinitely, and four report lines naming work no run could ever complete. The freeze's own justification, that the second copy appears in exactly one enumeration, held only for a server with an incremental one.

Four of the user's events existed on the server and in no local row. That is what a replica is for, and it is what the freeze cost.

## What landed

- **The line stops by itself.** Nothing in `itemize_fetches` changed: the twin now gets a row, so it leaves the pull plan the way any fetched item does. The test that proves it drives the reported shape end to end against a fake listing remote (complete snapshots, an empty checkpoint, an identity that only a body resolves): one `UID` under two hrefs mirrors as two items with two bodies, and the second run reports nothing at all and downloads nothing.

- **A link id is split, in one place** (`Kind::split_link_id`, `src/kind/mod.rs`). A minted key (`dup:<hint>#<handle>`, pimdir SPEC §9) is not an identity, and handing one to a backend whole would push `dup:…` as a `UID` and search IMAP for a `Message-ID` nothing carries. The split returns the hint, which is the identity both copies genuinely state, and the mint, which is the handle the key was minted on. The kind fallbacks (`alt:`, `hash:`) still answer no hint, and a hint carrying a `#` survives, the split taking the last one because a `Message-ID` may hold one (RFC 5322 `atext`) and a path segment may not. It is documented as the one legitimate place a key is parsed, the store parsing none, and both write paths take their identity from it: `LinkId` crosses the client seam in place of the bare hint.

- **A minted copy is named beside its twin, never over it** (`resource_id`, `src/dav/client.rs`). The old fallback re-derived a name from the body when the key was prefixed, and a duplicate's body carries the identity its twin already took, so both copies of a pushed pair would have been named `<uid>.ics`: the second `PUT` would not have been refused, it would have been applied to the resource already there, overwriting a synced event and reporting success. The name is now the sanitised hint, the sanitised mint, and the kind's extension, and a minted copy with no usable hint is named after the mint alone rather than after a body that hashes like its twin's.

- **A create answered with a handle the side already holds is rejected** (`HeldHandles`, `src/offline/remote.rs`). A server treating the `UID` as its key may answer a `PUT` by updating the resource that already holds it and returning that href, which RFC 6352 §6.3.2 forbids and nothing here can prevent, so it is caught on the way back: the enumerations of the run, less what they reported vanished, plus every handle a create was assigned. It is a floor rather than a proof (a full listing makes it the whole collection, an incremental one what changed), and it is the shape that matters, since two items on one handle make the next enumeration read one as vanished and propagate a delete of a resource nobody removed.

- **A refused duplicate says why** (`SyncReport::refused`, `RefusedDuplicate`). io-webdav names the `no-uid-conflict` precondition of RFC 4791 §5.3.2 and RFC 6352 §6.3.2, this crate reads it back out of the anyhow chain at the DAV seam (`is_duplicate_uid`, the peer of the `is_unsupported_report` the enumeration already matches on), and the report warns with the side, the collection and the `UID` instead of a bare 409. It repeats every run, which is correct and is the difference from the line this change removes: the run wrote nothing, the state is unresolved, and the line carries the one action that resolves it.

- **The ambiguity reporting is gone**, with the state it reported: `SyncReport::ambiguous`, `AmbiguousIdentity` and the `ambiguous` `--json` key, the `ambiguity` itemisation in both `itemize` and `itemize_single`, and the `Ambiguous` arm of `placement_hunks`, which the status enum no longer has. Nothing in `src/sync/hunk.rs` existed for it. The half of the requirement worth keeping, that this crate derives no duplicate rule of its own and never calls a collection invalid, is now visible in what it does not print: a collection holding one identity twice mirrors as two items and says nothing.

## Found on the way

`relay_targets` derived a target's `Message-ID` with `link.0.strip_prefix("mid:")`, a second hand-rolled parse of a link id, and one that has answered `None` for every item since mail link ids became bare `Message-ID`s. Every relayed append therefore went out with no hint, which an IMAP server without UIDPLUS cannot resolve. It now takes both halves from the same split as every other write path, which is the point of having one.

## Not in scope, as proposed

No repair verb: neverest does not delete a copy, does not re-`UID` one and does not offer to. No warning for a duplicate that syncs cleanly. The pull plan is not re-engineered; what changed is that the twin leaves it by getting a row.

## Verification

- 123 unit tests green (up from 120), `cargo clippy --all-targets` clean, `cargo fmt`. Also checked with `--no-default-features` plus `imap` alone, the build where no write can be refused with a `no-uid-conflict`.
- `one_identity_under_two_hrefs_mirrors_as_two_items_and_settles` is the reported case: two hrefs, one `UID`, no `sync-collection`; both copies stored with their own bodies, one keeping the identity bare and one minted on the href it came from, and a second run that reports nothing and fetches nothing.
- `a_minted_copy_is_addressed_beside_its_twin_rather_than_over_it` and `a_minted_copy_without_a_uid_is_never_named_after_its_body` pin the two halves of the naming trap; `a_create_answered_with_a_handle_the_side_holds_is_refused` and `a_vanished_handle_stops_being_held` pin the guard; `a_refused_duplicate_is_named_with_its_uid` pins the report line in both formats; `split_link_id` is covered per kind, fallback and separator included.
- `tests/duplicates.rs` is rewritten to the new outcome rather than deleted, being the regression this is judged on: one copy on A and two on B now leaves A holding both, the run after it is quiescent, deleting one copy on B removes exactly that copy on A, and a lost checkpoint re-appends nothing. It needs the two Stalwart instances and was **not** run in this session, docker being unavailable here; the shape it asserts is the one the driver-level reproduction proves against fakes.

## Dependencies

The three siblings are unpublished, so `[patch.crates-io]` points io-pimdir, io-replica and io-webdav at the working trees beside this one (`path = "../…"`), on the user's instruction, while the whole local tree is made testable end to end. The requirements in `[dependencies]` stay honest. Putting them back to git or crates.io once those crates release is a task on this change rather than a thing to remember.

Capabilities moved: `sync`.
