---
cairn: tasks
change: follows-storage-draft-17
---

# Tasks

Every command runs from the repository root as `CARGO_TARGET_DIR=~/code/pimalaya/.lanes/neverest/target CARGO_PROFILE_DEV_DEBUG=0 taskset -c 0,1,16,17 nix develop -c <cmd>` with `-j 4`. Git as frame.md §4: an unsigned conventional commit after each gated step, never push. Decisions and parked steps go to journal.md (frontmatter `cairn: change`), the end report to report.md.

Gate G0: `cargo fmt --check`; `cargo clippy --locked -j 4 --all-targets <set> -- -D warnings` over the 13 sets of .github/workflows/tests.yml; `cargo test --locked -j 4 --all-features --no-fail-fast`; `cargo test --locked -j 4 --no-default-features --features rustls-ring,imap,smtp --no-fail-fast`; `cargo deny check`; `cairn/verify.sh`. Base counts: inventory.md B.

## A to C: the move

- [ ] 1. Cargo.toml:83 rev `7f16b27da3b11a42b9d81a522a3e6fa5eeeed994`; Cargo.lock:1001 the io-pimdir source line alone, by hand (D1). Gate: `cargo metadata --locked` passes, `git diff --stat Cargo.lock` one line.
- [ ] 2. src/sync/driver/tests.rs:75-82 `a_store_version_tells_an_earlier_neverest_from_a_newer_one`: draft-15 and draft-16 earlier, `io_pimdir::sql::FORMAT_VERSION` not earlier, draft-18 not earlier. Gate: it fails before the edit (inventory B), G0 all green but `cargo deny`.
- [ ] 3. `cargo update -p url --precise 2.5.9` (url and form_urlencoded alone, D3). Gate: G0 green. Commit `build: follow io-pimdir 7f16b27 (storage-draft-17, search-draft-17)`.

## G1: a reset keeps local properties

- [ ] 4. src/cli/sync.rs tests: `a_reset_refuses_while_an_item_holds_a_property`, from the lane's repro (inventory G1): a based message, `add_property(seq, "pimdir/label", "keep")`, a plain reset fails naming `--force` and the label stays; a forced one drops it. Gate: fails on step 3's tree.
- [ ] 5. src/cli/sync.rs:808-867 `pending_work`: count the collection's items holding a property (`list_items` paged as `list_retained` is, `list_item_properties` per seq), named "N item(s) holding local properties". Gate: step 4 passes, G0. Commit `fix(sync): refuse a plain reset over items holding local properties`.

## E: inherited behaviour pinned

- [ ] 6. src/sync/driver/tests.rs: `a_capped_renumbered_mailbox_parks_on_one_absence` over `FakeImap` (two bound, one listed, `capped`): nothing written, `absences` names the rebuild, exit 3; with `--accept-absences` the binding drops. Gate: passes, and fails with `capped` left unstated on the rekey's pages (src/sync/driver/reconcile.rs:77). Commit `test(sync): pin the capped rekey park`.

## F: sync adoption

- [ ] 7. Test first, src/sync/driver/tests.rs `a_settled_collection_is_listed_once_a_run`: a writable QRESYNC `FakeImap` source synced twice, the second run sends one `SELECT` (`server.sent("SELECT")`); a pair twin over two fakes. Gate: fails today (two).
- [ ] 8. F1: src/sync/driver/spine.rs:708-719 and src/sync/driver/pair.rs:596-604 skip the push pass of a collection `settled_collections` names, read where that pass would start (in a pair, after the other side's pull). Gate: step 7 passes, G0, the Stalwart suites of step 15. Commit `perf(sync): leave a settled collection after its pull pass`.
- [ ] 9. F2 Gmail: src/gmail/run.rs:244 history; a label the run's history names nothing for and the store calls settled is left out of phase 1 (src/sync/driver/spine.rs:14). Test first: scripted Gmail, two labels, the second run lists neither. Gate: G0. Commit `perf(gmail): leave a label its history did not name`.
- [ ] 10. F2 DAV: src/dav/client.rs collection listing asks `{DAV:}sync-token` (RFC 6578 §4); a settled collection whose token equals its checkpoint's is not spined. Test first over tests/fakedav: the second run sends no `REPORT`. If io-webdav cannot ask the property, journal.md, park, an io-webdav ask. Gate: G0. Commit `perf(dav): leave a collection whose sync token did not move`.
- [ ] 11. F2 IMAP: does io-imap 0.12 encode `LIST "" "*" RETURN (STATUS (HIGHESTMODSEQ UIDVALIDITY))`? Read its source; yes: the same shape as 9 behind the server's `LIST-STATUS`; no: journal.md, an io-imap ask, parked.
- [ ] 12. F3: Gmail states `server_object_id = Some(handle)` on every member it names (src/gmail/client.rs:1798 `message_meta` callers, src/sync/remote.rs:606-621 for a source with `global_handles`). Test first, scripted Gmail: two messages of one `Message-ID`, other bodies, in two labels: the second body fetched, not linked. Gate: G0. Commit `fix(gmail): state the message id as the server object id`.

## H: performance (scratch only, D5)

- [ ] 13. Harness under ~/code/pimalaya/.pimdir-migration/neverest/perf: (a) in-process, a scratch patch of src/imap/fake.rs serving N mailboxes and an `#[ignore]` test timing `run_local` and counting `SELECT`s and whole loads; (b) the release binaries against a Stalwart seeded under the `servers` lock with a tenth of the Enron store's folders and their messages (folder tree and bodies read under the shared `store-517k` lock). Gate: an A/A of two builds of one tree within 1%.
- [ ] 14. Measure under the `measure` lock on CPU 15, hyperfine, binaries on ext4, HEAD (b080e14, its own draft-15 store) and the move interleaved, again after step 12: H1 no-op run, H2 first sync, H3 300 new messages in one folder, H4 the refresh after H3 and `sync --index-only` over a removed index.db. Thresholds: no measure of the move above HEAD by more than 5% (H2 10%); after F1 one listing per settled writable collection, after F2 none for a settled Gmail label or DAV collection (counts, exact); timings reported, no threshold. Tables in report.md, marked when other lanes were building.

## I: docs

- [ ] 15. Fold delta.md into cairn/spec/sync.md; log cairn/log/2026-MM-DD-follows-storage-draft-17.md; CHANGELOG.md [Unreleased]: a BREAKING line (a store of storage-draft-16 or earlier is refused, the start over path of proposal C; the index rebuilt), Fixed (G1, url advisory), Changed (F1 to F3); status `landed`. Gate: `cairn/verify.sh`, tests/docs.rs.

## J: tests beyond the unit suite

- [ ] 16. Under the `servers` lock: `./tests/pasta.sh servers`, then the 35 Stalwart and 17 Radicale tests (`./tests/pasta.sh cargo test --test <file> -- --ignored --test-threads=1`). Gate: all pass but the listed variant of tests/moves.rs (out of scope).
- [ ] 17. Cross-app: a store and index this build writes from the step 16 Stalwart account, read by the himalaya, cardamum, calendula and carillon builds of their lanes on 7f16b27 (list, search, show); a `set-flags` and a `move` himalaya queues, drained by neverest. Gate: every read answers, both rows applied.
- [ ] 18. Live, owner's grant only: `cargo test --test google -- --ignored --test-threads 1` and `--test msgraph`. Gate: 20 pass.
- [ ] 19. Once io-pimdir fixes R5, R6, R6b and K4 (needs-io-pimdir.md 1): its three app tests, each failing on 7f16b27 first.

## K: release, waiting on the owner

- [ ] 20. After io-pimdir 0.20.0 is published by its tag workflow: `io-pimdir = { version = "0.20.0", ... }`, the patch line gone, `cargo update -p io-pimdir`, G0, a CHANGELOG line. neverest's own release waits for the agreed order, pimdir v1 first, then neverest v1, and for every CLI being on the same io-pimdir revision.

## L: report

- [ ] 21. report.md: G1 first, then counts, measures, parked steps, what waits on the owner.
