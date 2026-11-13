---
cairn: log
change: neverest-follows-draft-11
date: 2026-11-13
---

# neverest follows draft-11

On io-pimdir cd33f0e (pushed, its crate still 0.11.0 and not on crates.io; pimdir d8800fb: storage-draft-11, sync-draft-11, search-draft-11), pinned by git in `[patch.crates-io]`, the dependency line staying `0.11`. Every store an earlier neverest wrote, a draft-10 one included, is refused as `store-version` with the reinstall message unchanged, and the search index is built again by the first refresh. No production line of neverest moved: the tree built on the new pin as it stood and its 508 unit tests passed before any test was touched. What follows is each rule that moved, read against how neverest uses it.

## What followed

- **The draft names.** `written_earlier` (src/offline/driver.rs) compares draft numbers against io-pimdir's `FORMAT_VERSION`, so a draft-10 store reads as an earlier neverest's with no edit, and a draft-12 one as a newer one's. The tests naming the stranded draft name draft-10 (below).
- **A move over a conflict** (pimdir SYNC §7; STORAGE §15.2 to §15.4). neverest stages no `Move` of its own: its one mutation is the overruling `Edit` of a one-way overwrite (`keep_store_bodies`), and its resolutions are queued `update`s stating `resolves` (src/conflict.rs). The rule reaches it through the drain alone: a queued `move` of an item in conflict is parked by the store, and neverest relays the row as it does any parked one, under `parked` in a sync's report and in `neverest drain`, the error naming the diverging body, the run exiting 3 as it already did for the conflict. The item stays in its collection and stays counted in `outstandingConflicts`. Nothing to adapt but the spec, the changelog and one test.
- **A mail `date`** (pimdir STORAGE §13). IMAP and Gmail name a message through io-pimdir's `derive_meta`, which writes the one shape the store takes. Graph is the one date neverest writes itself (`message_meta`, src/msgraph/client.rs): `sentDateTime` parsed as RFC 3339, brought to UTC and written at seconds precision under `Z`, the shape the schema holds, which `a_row_and_its_body_derive_one_key` already ties to io-pimdir's own derivation of the same message. neverest's own scope reads (`keep_in_scope`, Graph's `in_scope`, the relay's `covered`) all go through `PimdirScope::contains`, which now reads a date of another shape as none, so they agree with the store without an edit.
- **`unconfirm_action`** runs the canonical statement. neverest hands it the detail with no prefix (`unconfirm_submit`) and reads the row back under `unconfirmed: `, as before: `an_interrupted_send_is_parked_unconfirmed`, the dispositions test and tests/intents.rs pass unchanged.
- **Search.** neverest refreshes the index and never queries it: it builds no `PimdirSearchReads` (no struct literal broke on `mark`), reads no coverage and no `is_complete`. An index of search-draft-10 is rebuilt by the first refresh, through the same `PimdirIndex::open` and `refresh`.
- **A restore** (pimdir STORAGE §8). neverest takes no backup and restores none. The dry-run replica is a clone of the store's files made under the run's lock and thrown away, never put back over a store, so the restore's order does not reach it; it still carries local/, the lock files and index.db, which a backup of §8 leaves out and a throwaway copy may keep.
- **The attachments of a body** (pimdir STORAGE §11.1, §14.3, Annex A.1). neverest writes the store through io-pimdir's write alone, so the stand-ins follow the crate: none for a body entering under a `Message-ID` another message of the account holds, and the last part of a multipart no closing delimiter ends read as a part.
- **A capped source** (3451138). Unchanged by this follow. io-pimdir cd33f0e reads `capped` in a sync alone: `PimdirRekey` does not read `PimdirRemoteSnapshot::capped` (src/rekey.rs has not moved since 0.11.0), and pimdir SYNC §8 of sync-draft-11 still states no rule for a capped listing. neverest still states `capped` on the rekey's pages (`rebuild_collection`), and a rekey of a capped collection is still guarded by its own park alone.

## Tests

Changed because they named a draft whose place moved, each seen failing on io-pimdir 0.11.0 (the patch line taken out of Cargo.toml for one run, then both Cargo files restored from copies kept aside and compared, never through git):

- src/offline/driver.rs `a_store_version_tells_an_earlier_neverest_from_a_newer_one`: draft-10 is an earlier neverest's, draft-12 a newer one's, where draft-11 was the newer one. On 0.11.0: `assertion failed: written_earlier(&refused(Some("storage-draft-10")))`.
- src/offline/index.rs `an_index_of_an_earlier_draft_is_rebuilt`: restamps `search-draft-10`, the index this move strands, where it restamped draft-07. On 0.11.0 the index is current and nothing is folded again.
- tests/drain.rs `a_store_an_earlier_neverest_wrote_asks_for_a_reinstall`: restamps `storage-draft-10`. On 0.11.0 the drain opens the store and exits 0.

New:

- src/offline/driver.rs `a_queued_move_over_a_conflict_parks_and_the_decision_still_waits`: a conflicted card, a `move` queued on it, the drain and the report as a sync runs them. Nothing applied, one `move` parked whose error holds the diverging body's hash, one conflict still waiting, the card still in its address book and none in the target. On 0.11.0: `[DrainedQueue { collection: "contacts", applied: 1 }]`, the move applied.

The server suites (tests/moves.rs, tests/scope.rs and the others) compile and were not run.

## Evidence

Each as `nix develop --command <cmd>`, on 2026-11-13:

- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings` with `--all-features`, with the defaults, with `--no-default-features`, and with `--no-default-features` plus each of `search`, `rustls-ring,dav,vendored`, `rustls-ring,imap`, `rustls-ring,gmail`, `rustls-ring,msgraph`: clean, all eight.
- `cargo check --all-features --tests`: clean.
- `cargo test --all-features --bin neverest`: 509 passed (508 before, on the old pin and on the new one before the tests moved).
- `cargo test --all-features --test accounts --test check --test drain --test intents --test usage`: 7, 1 (2 ignored), 9 (1 ignored), 9 and 2 passed.

## Not done

- **A rekey of a capped collection** is still protected by its park alone, as cairn/log/2026-11-13-a-capped-source-says-so.md left it: the standard's, then the engine's.
- **A parked `move` is not retried by neverest** once its conflict is settled: the row leaves by its producer, retried or cancelled, as any parked row does.
- **A date of another shape keeps its sort key.** io-pimdir's `PimdirMailSummary::sort_key` and `link_id` read the stated `date` as written while `PimdirSummary::stored` holds it as none, so a connector stating one would leave a row dated none under a sort key and an `alt:` key naming the text. No neverest connector states one (Graph's writer cannot leave the shape for a year from 0001 to 9999 stated in UTC), so nothing was worked around; it is io-pimdir's to settle.
- No server test moves an item in conflict; ./tests/pimdir.sh, the Stalwart, Radicale and live suites wait for the owner, after the stores are created again.
- The pin goes back to a crates.io version once io-pimdir releases the drafts.

## Capabilities moved

- sync (cairn/spec/sync.md): modified the header (the drafts and the pin), "A store the format outgrew is refused with its remedy" (its scenario names `storage-draft-10`), "Neverest is the store's sole owner and drains the queue first" (a queued `move` over a conflict parks, with its scenario), "A Graph mail is dated by its `Date`" (the shape the store takes), "The derivations are the format's" (the walk of an unclosed multipart and an entity's first occurrence), "A landed body stands for its attachments" (a `Message-ID` another message holds), "A send that may have been delivered is never sent again" (the statement writes the prefix).
