---
cairn: tasks
change: follows-storage-draft-17
---

# Tasks

Every command runs from the repository root as `CARGO_TARGET_DIR=~/code/pimalaya/.lanes/neverest/target CARGO_PROFILE_DEV_DEBUG=0 taskset -c 0,1,16,17 nix develop -c <cmd>` with `-j 4`. Git as frame.md §4: an unsigned conventional commit after each gated step, never push. Decisions and parked steps go to journal.md (frontmatter `cairn: change`), the end report to report.md.

Gate G0: `cargo fmt --check`; `cargo clippy --locked -j 4 --all-targets <set> -- -D warnings` over the 13 sets of .github/workflows/tests.yml; `cargo test --locked -j 4 --all-features --no-fail-fast`; `cargo test --locked -j 4 --no-default-features --features rustls-ring,imap,smtp --no-fail-fast`; `cargo deny check`; `cairn/verify.sh`. Base counts: inventory.md B.

Closed 2026-11-26: the steps below were run on 2026-11-23 (report.md, log 2026-11-23-follows-storage-draft-17) and checked against ef809a9; what targeted the draft-17 pins and the 0.20.0 release is superseded by io-pimdir 1.0.0 (log 2026-11-26-io-pimdir-1-0).

## A to C: the move

- [x] 1. Cargo.toml:83 rev `7f16b27da3b11a42b9d81a522a3e6fa5eeeed994`; Cargo.lock:1001 the io-pimdir source line alone, by hand (D1). Gate: `cargo metadata --locked` passes, `git diff --stat Cargo.lock` one line. Done on 2026-11-23 (then cc3021b, 2314084, 17d4ea8); superseded: io-pimdir 1.0.0 from crates.io, no `[patch]`.
- [x] 2. src/sync/driver/tests.rs:75-82 `a_store_version_tells_an_earlier_neverest_from_a_newer_one`: draft-15 and draft-16 earlier, `io_pimdir::sql::FORMAT_VERSION` not earlier, draft-18 not earlier. Gate: it fails before the edit (inventory B), G0 all green but `cargo deny`. Superseded: the test now names draft-17 and draft-18 earlier and `storage-v2` later (io-pimdir-1-0).
- [x] 3. `cargo update -p url --precise 2.5.9` (url and form_urlencoded alone, D3). Gate: G0 green. Commit `build: follow io-pimdir 7f16b27 (storage-draft-17, search-draft-17)`. Cargo.lock holds url 2.5.9.

## G1: a reset keeps local properties

- [x] 4. src/cli/sync.rs tests: `a_reset_refuses_while_an_item_holds_a_property`, from the lane's repro (inventory G1): a based message, `add_property(seq, "pimdir/label", "keep")`, a plain reset fails naming `--force` and the label stays; a forced one drops it. Gate: fails on step 3's tree. In src/cli/sync.rs.
- [x] 5. src/cli/sync.rs:808-867 `pending_work`: count the collection's items holding a property (`list_items` paged as `list_retained` is, `list_item_properties` per seq), named "N item(s) holding local properties". Gate: step 4 passes, G0. Commit `fix(sync): refuse a plain reset over items holding local properties`.

## E: inherited behaviour pinned

- [x] 6. src/sync/driver/tests.rs: `a_capped_renumbered_mailbox_parks_on_one_absence` over `FakeImap` (two bound, one listed, `capped`): nothing written, `absences` names the rebuild, exit 3; with `--accept-absences` the binding drops. Gate: passes, and fails with `capped` left unstated on the rekey's pages (src/sync/driver/reconcile.rs:77). Commit `test(sync): pin the capped rekey park`.

## F: sync adoption

- [x] 7. Test first, src/sync/driver/tests.rs `a_settled_collection_is_listed_once_a_run`: a writable QRESYNC `FakeImap` source synced twice, the second run sends one `SELECT` (`server.sent("SELECT")`); a pair twin over two fakes. Gate: fails today (two).
- [x] 8. F1: src/sync/driver/spine.rs:708-719 and src/sync/driver/pair.rs:596-604 skip the push pass of a collection `settled_collections` names, read where that pass would start (in a pair, after the other side's pull). Gate: step 7 passes, G0, the Stalwart suites of step 15. Commit `perf(sync): leave a settled collection after its pull pass`. src/sync/driver/reconcile.rs `owes_no_push`, `a_run_reads_the_settled_collections_once`.
- [x] 9. F2 Gmail: src/gmail/run.rs:244 history; a label the run's history names nothing for and the store calls settled is left out of phase 1 (src/sync/driver/spine.rs:14). Test first: scripted Gmail, two labels, the second run lists neither. Gate: G0. Commit `perf(gmail): leave a label its history did not name`. src/gmail/client.rs `quiet_labels`.
- [x] 10. F2 DAV: src/dav/client.rs collection listing asks `{DAV:}sync-token` (RFC 6578 §4); a settled collection whose token equals its checkpoint's is not spined. Test first over tests/fakedav: the second run sends no `REPORT`. If io-webdav cannot ask the property, journal.md, park, an io-webdav ask. Gate: G0. Commit `perf(dav): leave a collection whose sync token did not move`. src/dav/client.rs `quiet_collections`, tests/pair.rs `a_book_whose_sync_token_did_not_move_is_not_listed`.
- [ ] 11. F2 IMAP: does io-imap 0.12 encode `LIST "" "*" RETURN (STATUS (HIGHESTMODSEQ UIDVALIDITY))`? Read its source; yes: the same shape as 9 behind the server's `LIST-STATUS`; no: journal.md, an io-imap ask, parked. Parked 2026-11-23: io-imap 0.12.0, still locked at ef809a9, sends none; an io-imap ask, a performance gain only, not carried as neverest work.
- [x] 12. F3: Gmail states `server_object_id = Some(handle)` on every member it names (src/gmail/client.rs:1798 `message_meta` callers, src/sync/remote.rs:606-621 for a source with `global_handles`). Test first, scripted Gmail: two messages of one `Message-ID`, other bodies, in two labels: the second body fetched, not linked. Gate: G0. Commit `fix(gmail): state the message id as the server object id`. Closed with no production change: the linking was not reproduced; src/sync/driver/gmail_bodies.rs `two_messages_of_one_message_id_keep_their_own_bodies` pins the property.

## H: performance (scratch only, D5)

- [x] 13. Harness under ~/code/pimalaya/.pimdir-migration/neverest/perf: (a) in-process, a scratch patch of src/imap/fake.rs serving N mailboxes and an `#[ignore]` test timing `run_local` and counting `SELECT`s and whole loads; (b) the release binaries against a Stalwart seeded under the `servers` lock with a tenth of the Enron store's folders and their messages (folder tree and bodies read under the shared `store-517k` lock). Gate: an A/A of two builds of one tree within 1%. Run 2026-11-23 (report.md, Measures).
- [x] 14. Measure under the `measure` lock on CPU 15, hyperfine, binaries on ext4, HEAD (b080e14, its own draft-15 store) and the move interleaved, again after step 12: H1 no-op run, H2 first sync, H3 300 new messages in one folder, H4 the refresh after H3 and `sync --index-only` over a removed index.db. Thresholds: no measure of the move above HEAD by more than 5% (H2 10%); after F1 one listing per settled writable collection, after F2 none for a settled Gmail label or DAV collection (counts, exact); timings reported, no threshold. Tables in report.md, marked when other lanes were building.

## I: docs

- [x] 15. Fold delta.md into cairn/spec/sync.md; log cairn/log/2026-MM-DD-follows-storage-draft-17.md; CHANGELOG.md [Unreleased]: a BREAKING line (a store of storage-draft-16 or earlier is refused, the start over path of proposal C; the index rebuilt), Fixed (G1, url advisory), Changed (F1 to F3); status `landed`. Gate: `cairn/verify.sh`, tests/docs.rs. Log 2026-11-23-follows-storage-draft-17; the spec's draft citations since moved to the v1.0.0 releases, the CHANGELOG folded into 1.0.0.

## J: tests beyond the unit suite

- [x] 16. Under the `servers` lock: `./tests/pasta.sh servers`, then the 35 Stalwart and 17 Radicale tests (`./tests/pasta.sh cargo test --test <file> -- --ignored --test-threads=1`). Gate: all pass but the listed variant of tests/moves.rs (out of scope). 51 of 52 (report.md); the listed variant stays with a-move-into-a-same-id-message-keeps-both.
- [x] 17. Cross-app: a store and index this build writes from the step 16 Stalwart account, read by the himalaya, cardamum, calendula and carillon builds of their lanes on 7f16b27 (list, search, show); a `set-flags` and a `move` himalaya queues, drained by neverest. Gate: every read answers, both rows applied. Done on drop 3 (report.md); repeated on the 1.0 candidate by the live night of 2026-11-25.
- [x] 18. Live, owner's grant only: `cargo test --test google -- --ignored --test-threads 1` and `--test msgraph`. Gate: 20 pass. Run by the live night of 2026-11-25 on the 1.0 candidate: google 8/8, msgraph 15/15 (.lanes/live/report-3.md §2).
- [x] 19. Once io-pimdir fixes R5, R6, R6b and K4 (needs-io-pimdir.md 1): its three app tests, each failing on 7f16b27 first. src/sync/driver/tests.rs `a_renumbered_mailbox_binds_a_pending_create_to_the_member_it_names`, `a_renumbered_mailbox_binds_an_offered_item_to_the_member_it_names`, `a_delivered_create_a_rebuild_names_is_reported_created`, `a_queued_move_into_an_identity_the_other_endpoint_binds_joins_it`.

## K: release, waiting on the owner

- [x] 20. After io-pimdir 0.20.0 is published by its tag workflow: `io-pimdir = { version = "0.20.0", ... }`, the patch line gone, `cargo update -p io-pimdir`, G0, a CHANGELOG line. neverest's own release waits for the agreed order, pimdir v1 first, then neverest v1, and for every CLI being on the same io-pimdir revision. Superseded: 0.20.0 landed on 2026-11-24, then io-pimdir 1.0.0 (pimdir v1) in ef809a9.

## L: report

- [x] 21. report.md: G1 first, then counts, measures, parked steps, what waits on the owner.
