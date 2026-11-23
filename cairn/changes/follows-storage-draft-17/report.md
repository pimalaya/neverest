---
cairn: change
id: follows-storage-draft-17
status: active
created: 2026-11-23
---

# Report: neverest follows storage-draft-17

Lane run of 2026-11-23 under .lanes/RUN.md; the lead committed each point, signed. Builds on CPUs 0, 1, 16, 17, measures on CPU 15. journal.md holds the timed record.

## G1 first: a reset lost local properties (silent loss, fixed)

`sync --reset` without `--force` dropped every local property of the account's items (a `pimdir/label` among them), which no server holds: `pending_work` never counted them, and the store drops a property with the last row of its seq. A plain reset now refuses, naming the items holding one and `--force`. Fix in src/cli/sync.rs, test `a_reset_refuses_while_an_item_holds_a_property` (red before the fix). No other loss was found on neverest's surface.

## Where it stands

- io-pimdir: requirement `0.20.0`, patched to git 17d4ea8 (drop 3, the release candidate); earlier pins 7f16b27, cc3021b, 2314084.
- Suite all features: 740 passed, 0 failed, 72 ignored (base 727); `rustls-ring,imap,smtp`: 424/0/51 (base 415). Clippy clean on the 13 CI feature sets, fmt clean, `cargo deny` green (url 2.5.9 closes RUSTSEC-2026-0592, red at the base too), cairn/verify.sh conformant.
- Stalwart and Radicale suites: 51 of 52 pass; the failure is the listed variant of tests/moves.rs, left out by the plan (io-imap has no `EMAILID`). tests/multikind.rs and tests/merger.rs had been stale since the 2026-11-20 contract rulings; their tests were updated.
- Step 19: four tests pin io-pimdir drop 2's fixes, each red on the 7f16b27 engine (R6a and R6b sent an APPEND, R5 emitted no `Created`, K4 minted a `dup:` item).
- Step 17, cross-app: neverest wrote /home/soywod/code/pimalaya/.lanes/neverest/crossapp/store on drop 3 (account `crossapp`: three messages, one card, one event with an alarm); the other lanes read it; himalaya queued a `set-flags` and a `move`; one neverest sync applied both, on Stalwart and in the store, and the next sync reported nothing.

## What changed for users

- **BREAKING**: a store of storage-draft-16 or earlier is refused, never migrated; CHANGELOG.md gives the start over path (sync with the old neverest until nothing is queued, move the store away, `neverest init`, `neverest sync`). The first refresh rebuilds the index.
- Fixed: the reset over local properties (G1); the url advisory; a queued move reporting its create twice when the collection it enters was itemized first (found by the himalaya rehearsal).
- A malformed queue row is parked with its error and the drain goes on, exit 3 (io-pimdir drop 2).
- F1: a settled writable collection is listed once a run, solo and in a pair.
- F2: a Gmail label the account history shows unchanged is not listed, nor a DAV collection whose sync token did not move; the Gmail label's checkpoint still advances, in one write.
- The capped rekey parks on a single absence (io-pimdir's behaviour, now pinned by a test and the spec).

## Measures (step 14)

Fixture: a local Stalwart seeded from every tenth folder of the 517k Enron store (331 folders); the test account's limit refused 20,688 of 46,928 appends, so 27,140 items after H3. Binaries: base a5875c2 (b080e14, its draft-15 store), move 6b9cc09 (cc3021b, no F), now 15d6a5e (F1, F2). hyperfine, interleaved, three copies on ext4, CPU 15, 18:28 to 18:38 while other lanes built. Means in seconds (standard deviation):

| measure | base | move | now | now, A/A copy |
| --- | --- | --- | --- | --- |
| H1 no-op, 10 rounds | 2.953 (0.045) | 2.126 (0.048) | 1.969 (0.041) | 1.980 (0.045) |
| H2 first sync, 3 rounds | 40.1 (10.5) | 34.5 (6.5) | 29.7 (3.3) | 27.8 (1.2) |
| H3 300 new in one folder, 3 rounds | 3.50 (0.20) | 2.63 (0.11) | 2.46 (0.14) | 2.52 (0.15) |
| H4 `sync --index-only` over a removed index.db, 3 rounds | 5.62 (0.39) | 4.75 (1.00) | 5.13 (1.28) | 5.82 (2.24) |

- The move is faster than HEAD on every measure (H1 -28%, H2 -14%, H3 -25%, H4 -15%); the thresholds hold.
- Only H1 meets the 1% A/A check (0.6%); H2 and H4 are noisy (rootless docker on a shared machine): re-run in a quiet window.
- F1 read `settled_collections` once a collection (0.8 ms a read at 3,313 collections, so seconds a run on a large account); it now reads once a run that changes nothing, and again only after a pull or push changed the store (test `a_run_reads_the_settled_collections_once`: 3 reads to 1 over three mailboxes). A version reading strictly once failed tests/scope.rs `a_scoped_move_whose_answer_was_lost_is_made_once` 2 times in 10 Stalwart runs (likely a set gone stale after another collection's pass changed the store, not proven); the re-read version passed 5 of 5.
- Listing counts come from the tests: one `SELECT` per settled writable collection, solo and pair; no `REPORT` for an unchanged DAV book; one history read for unchanged Gmail labels.

## Parked

- Step 11, F2 over IMAP: io-imap 0.12.0 sends no `LIST ... RETURN (STATUS ...)` (RFC 5258, RFC 5819); needs io-imap.
- Step 12, F3 (Gmail's message id as the server object id): the linking it prevents was not reproduced at cc3021b; `two_messages_of_one_message_id_keep_their_own_bodies` pins the property, production unchanged.
- Step 18, live tests: the owner's grant.
- A run whose collection filter takes a move's target but not its source reports no add for that move; the run taking the source reports it.

## Waiting on the owner

1. io-pimdir 0.20.0 published by its tag workflow, then step 20 (the patch line removed, `cargo update -p io-pimdir`, G0, a CHANGELOG line).
2. The pimdir tags, then neverest v1 after pimdir v1, every CLI on the same io-pimdir revision.
3. Defaults taken: D2, the reset refusal over local properties (`--force` drops them); F1 and F2 delay a server change made after a collection's pull to the next run, never drop it; the change stays `active` while step 20 is open.
