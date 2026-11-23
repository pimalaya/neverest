---
cairn: change
change: follows-storage-draft-17
---

# Inventory, 2026-11-23

Measured today unless marked "read from code, unverified". Scratch: ~/code/pimalaya/.pimdir-migration/neverest (src: HEAD 99384ff with the pin moved; base: HEAD as is; repro, repro-base: the G1 test added; logs/; gate.sh runs every step below with CPUs 0,1,16,17, `-j 4`).

## A. Dependency

- Cargo.toml:83 is the one edit: rev b080e14 to 7f16b27.
- `cargo update -p io-pimdir` (also with `--precise`, also with `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=allow`) moves Cargo.lock:1001 and five `windows-sys` edges, 0.61.2 to 0.52.0 (errno, rustix, rustls-platform-verifier, tempfile, winapi-util). The source line edited alone passes `cargo metadata --locked`; every build below ran `--locked` on it.
- io-pimdir's Cargo.toml between the two revisions: only rustix gains `mm`. Features at both: `mail`, `contact`, `calendar` gate src/search/extract.rs alone.
- Already in b080e14, so nothing new to neverest: `LandPlacement`, `server_object_id`, `SetProperty`, `PimdirBodyFlush`, `WRITE_CHUNK` 4,096, `bindings.owes` and `items.lacks`, the capped rekey park (2b95e06, 2026-11-13). New since: storage-draft-17, search-draft-17, `settled_collections` (0e369bb), the load and refresh speedups. `PimdirError::code` answers the same 52 codes at both.

## B. Build and tests

| | HEAD (b080e14) | 7f16b27 |
| --- | --- | --- |
| compile, every set | clean | clean |
| `cargo test --locked --all-features` | 727 passed, 0 failed, 72 ignored | 726 passed, 1 failed, 72 ignored (160 s) |
| `--no-default-features --features rustls-ring,imap,smtp` | 415, 0, 51 | 414, 1, 51 |
| clippy, 13 CI sets, `-D warnings` | | all clean (117 s) |
| `cargo fmt --check` | | clean |
| `cargo deny check` | advisories FAILED | advisories FAILED |

- The failure: `sync::driver::tests::a_store_version_tells_an_earlier_neverest_from_a_newer_one`, src/sync/driver/tests.rs:82, `assertion failed: !written_earlier(&refused(Some("storage-draft-15")))`.
- Ignored, 72: live 20 (google 7, msgraph 13), Stalwart 35, Radicale 17 (multikind needs both). tests/without_uidplus.rs ran against maddy, 2 passed.
- `cargo deny`: RUSTSEC-2026-0592, url 2.5.8. `cargo update -p url --precise 2.5.9` moves url and form_urlencoded (1.2.2 to 1.2.3) alone; url 2.5.9 keeps rust-version 1.63 and `idna ^1.1.0` (crates.io index).

## C. Draft names

src/sync/driver/tests.rs:50 (draft-99), :75-82; src/sync/index.rs:241 (search-draft-99), :263 (search-draft-11); tests/queue.rs:383 (draft-99), :403 (draft-11): all but :82 still hold. Stale text: cairn/spec/sync.md:11 (names search-draft-15, b080e14 being search-draft-16 already), :1488 (per-kind summary tables), :2436 (capped rekey, against pimdir SYNC.md:542).

## D. neverest's own SQL

src/sync/storage.rs:151-176 (`register_syncing`: `sql::LOAD_CHECKPOINT`, `sql::UPSERT_CHECKPOINT` on `sources`, its own connection); src/sync/driver/upkeep.rs:361-362 (`VACUUM INTO`); tests only: src/cli/sync.rs:1059-1062 (`capabilities`), src/sync/driver/tests.rs:47-52, :533-537, src/sync/index.rs:207-208 (`DROP INDEX placement_by_link`), :228-230, tests/queue.rs:304-305, :368-370, :432-444. None names a moved table; all pass at 7f16b27.

## G1. A reset drops local properties

repro/src/cli/sync.rs `repro_a_reset_refuses_while_an_item_holds_a_label` (a based message, `add_property(seq, "pimdir/label", "keep")`, `reset_account(.., false)`): fails at 7f16b27 and at HEAD, "the plain reset went through, the label left: []" (logs/repro-reset-label.log, repro-reset-label-base.log). Cause, read from code: `pending_work` (src/cli/sync.rs:808-867) counts conflicts, retained items, deletes and unpushed changes; io-pimdir's trigger `items_drop_properties` (spec/migrations/storage/0001_init.sql) drops a property with the last row of its seq.

## F. Sync surface, read from code, unverified

- Rekey: IMAP alone answers `Listed::Rekey` (src/imap/backend.rs:159, 178, 249, 306); Graph bails (src/msgraph/client/mail.rs:1646). Every IMAP listing fetches `FLAGS` (src/imap/backend.rs:403-421, 506-534) and the remote states them known (src/sync/remote.rs:542-544, 618); R4's guard, `a_renumbered_mailbox_is_rekeyed_before_it_is_listed` (src/sync/driver/tests.rs:730), passes at 7f16b27 (measured). The rekey remote carries no bound set (src/sync/driver/reconcile.rs:76-77), so its listing names every member.
- COPYUID: src/imap/backend.rs:1492 `moved_uids`, src/sync/remote.rs:1778 `relocated` set `assigned`.
- Gmail handles are account-wide message ids (src/client.rs:762 `global_handles`); every meta states `server_object_id: None` (src/sync/remote.rs:193, 895, 932; src/sync/driver/gmail_bodies.rs:73).
- io-imap 0.12.0 has no `EMAILID` nor `X-GM-MSGID` fetch item (its scanner only reads one back, io-imap src/send.rs:1363).
- The DAV client sends a sync token only in its `sync-collection` body (src/dav/client.rs:1953, 2046), none in its collection listing.

## H. Hot paths, read from code, unverified

Per collection and run, the solo spine reads the collection whole in `load_side` (src/sync/driver/spine.rs:647), in each `sync_side` pass, pull then one to five push passes (src/sync/driver/reconcile.rs:134-140; spine.rs:708-719), and in phase 2 (`projection_view`, spine.rs:604); a writable side is listed by each pass (neverest log 2026-11-17-fewer-reads-a-run, Left; CLI analysis NEV-9: "loaded whole about nine times"). io-pimdir's measures (its logs of 2026-11-23): a whole load of 11,896 bindings 144 to 91 ms; no-op at 517k 89.2 to 67.6 ms, 18.6 skipping settled folders; 300 new messages 234 to 212 ms; a 300-message refresh 617 to 189 ms; a 517k rebuild 121.8 to 92.2 s. The draft-15 HEAD cannot open the draft-17 517k fixture, hence a server-seeded fixture for both.

## Open lines triaged

- neverest cairn/changes: a-move-into-a-same-id-message-keeps-both (listed variant, io-imap: out); an-imap-move-copies-without-move, the-contract-audit-rulings, the-v1-review (landed; NEV-X2 and the ../cli path left, outside the store); connector-listings-fail-rather-than-shrink (409 token, Graph shared calendar live check: out); gpeople-batch-get (live lines: out); jmap-sources (draft, after the next major: out); tests-inspect-with-the-linked-pimdir (Radicale run: step 16); the-docs-cite-storage-and-sync (dependency lines: step 15).
- neverest cairn/log since 2026-11-13, Left: the capped rekey (closed upstream on 2026-11-13, spec stale: G3); People and Graph one-page rounds (still so: src/gpeople/client.rs:205-235, src/msgraph/client/contacts.rs:108, calendar.rs:142; D6); Gmail `remove` (ruling: out); the store-only flag reset (owner's: out); the rest outside the store.
- CLI analysis 2026-11-15: NEV-1 to NEV-19 each logged on 2026-11-17; only NEV-9 and NEV-12 left lines, above.
- Ledgers: cli-alignment N0 (live quota) and X18 (himalaya's): not neverest's store; live-testing N27 landed (log 2026-11-15-neverest-follows-draft-13).
- pimdir: the-freeze-plan/tasks.md:69, an owner write registering a source with no checkpoint (needs-io-pimdir 2); audit-2026-11-12 N1, N2 closed.
- `gh issue list -R pimalaya/neverest`: none open.
