---
cairn: log
change: neverest-follows-draft-10
date: 2026-11-13
---

# neverest follows draft-10

On io-pimdir 0.11.0 from crates.io (the tree of 49e1fd5, pimdir ae8b231: storage-draft-10, sync-draft-10, search-draft-10), the git patch dropped. Every store an earlier neverest wrote, a draft-09 one included, is refused as `store-version` with the reinstall message unchanged, and the search index is built again by the first refresh.

## What landed

- **The build** (22 sites, 6 of them outside tests): `relocated` on every `PimdirPushResult` (src/offline/remote.rs), `enqueue_with_body` where `enqueue_with_body_checked` stood (src/conflict.rs), the `Unconfirmed` and `Reset` statuses (src/cli/drain.rs), and in the tests `base` on a `set-flags`, `if_match` and `resolves` on a `remove`, `PimdirEnqueued` where an enqueue answered an id, a receipt's optional collection.
- **A remove relocates or deletes, and says which** (pimdir SYNC §4, §5; audit line Y2). `PimRemote::push` asks each remove's target, before the batch and one target after another, whether it holds the identity (`targets_holding`, through `Client::find_message`: a `Message-ID` on IMAP and Graph mail, never under a minted key). A target holding it has the member deleted from its own collection, on the change's `if_match`, the outcome stating no relocation; any other is relocated as before and answers `relocated: true`, which is what delivers the target's create. A target that cannot be asked rejects the change.
- **An append its server does not name is accepted with no handle** (pimdir SYNC §4; Y2). `add_message_stream` (src/imap/backend.rs) answers `None` where it failed, with no `APPENDUID` and no `Message-ID` a search finds; `Client::add_item_stream` answers `Option<WrittenItem>`, and `PimRemote::append` accepts with `assigned: None`, which the engine records delivered. The relay (`relay_one`), binding its copy around the engine, still fails there.
- **The owner's files under local/** (pimdir STORAGE §3, §6, §18; F14). src/offline/local.rs creates the store's local/ `0700` and its files `0600`, refusing a link or a file under the name. neverest.json (src/offline/state.rs), sync.lock (src/cli/sync.rs) and the `submitting` mark (src/offline/submit.rs) live there. A neverest.json beside the database is refused as an earlier layout (`store-layout`). The dry-run replica copies the directory tree, so it carries local/.
- **The unconfirmed intent** (pimdir STORAGE §13, §15.2; F16). A send that may have been delivered is parked through `unconfirm_action`, the store writing the reserved prefix `unconfirmed: `; neverest's own `uncertain: ` is gone. `SubmitEntry::uncertain` is `unconfirmed`, in the JSON and its schema, and the entry's `error` reads as the row does.
- **The report** (pimdir SYNC §5, §6; Y7). `waiting` lists the creates the last pass of each collection left waiting (`PimdirSyncReport::waiting`), by key and reason, printed under `Waiting`, no warning and no exit code. `disagreed` lists the members named apart from a create of their side (`PimdirSyncEvent::Disagreed`), a warning. `Abandoned` is logged only: no neverest command stages an `Abandon`, so no run can report one. The code `lock-replaced` is relayed; `scope-invalid`, `store-reset`, `action-not-intent` and `backup-occupied` join the codes neverest cannot meet.
- **A create naming no member**. `bodiless` (src/offline/driver.rs) plans no body fetch under a provisional handle, and `pending_work` (src/cli/sync.rs) counts a delivered create as staged work, so `sync --reset` refuses over it without `--force`. Both were tested before being changed, and both tests failed: the plan named `\u{1}delivered@x` and `\u{1}pending@x`, and the reset dropped the delivered create.
- **An intent stating an unknown version stays pending** (pimdir STORAGE §15.3). `states_unknown_version` (src/offline/mod.rs, over io-pimdir's `payload_outcome`) keeps such a row out of `submit::pending`, `invitation::pending` and `collection::pending`: never performed, no attempt counted, never parked.
- The IMAP fake (src/imap/fake.rs) reads a synchronising literal, so a test can append through it; it files nothing and answers no `APPENDUID`.

## Tests

New, each failing with its change put back (the first seven by mutating the tree and running them, the next two before their fix was written):

- src/offline/driver.rs: `a_remove_relocating_its_member_says_so`, `a_remove_into_a_target_holding_the_identity_deletes`, `a_remove_under_a_minted_key_relocates_unasked`, `an_append_answered_no_uid_is_accepted_with_no_handle`, `a_create_appended_without_a_uid_waits_and_is_not_appended_again` (the engine over the fake: appended once, reported `delivered`), `an_interrupted_send_is_parked_unconfirmed`, `an_intent_of_an_unknown_version_stays_pending`, `a_create_naming_no_member_is_owed_no_body_fetch`.
- src/cli/sync.rs: `a_reset_refuses_while_a_delivered_create_waits`, `the_sync_lock_is_kept_under_local`.
- src/offline/state.rs: `the_sidecar_is_kept_under_local`, `a_sidecar_beside_the_database_is_refused_as_an_earlier_layout`. src/offline/local.rs: three. src/offline/submit.rs: the mark under local/, the reason under the reserved prefix. src/error.rs: `a_replaced_lock_carries_its_code`.
- New surface with no earlier code to fail on: `a_member_named_apart_from_its_create_is_reported_once`, `a_dry_run_replica_carries_the_owners_state`.
- tests/*.rs follow (local/ paths, `unconfirmed`, the `Unconfirmed` status, a store of `storage-draft-09` as the earlier one). They compile and were not run.

## Evidence

`cargo fmt --check` clean; `cargo clippy --all-features --all-targets -- -D warnings` clean; `cargo check --all-features --tests` clean; `cargo test --all-features --bin neverest` 543 passed (525 before). `cargo clippy --no-default-features`, with `search` and with `rustls-ring,dav,vendored` clean; with `rustls-ring,imap` it fails on an unreachable arm of `Client::find_message` (src/client.rs), which this change did not touch.

## Not done

- **Y1, the Graph pass still states gone what it no longer lists** (src/msgraph/client/mail.rs, `pass`). Stating only the rows Graph marks removed was tried and withdrawn: `an_interrupted_round_resumes_its_band_and_passes_again` then keeps `2026-09-10`, a message a band page listed and the server dropped before the pass. That page stamped its binding, so the engine, which finds absent only an unstamped binding, never drops it, and the delta link the pass makes reports no removal older than itself: the row stays until a full sync. pimdir SYNC §4 forbids the statement and says a checkpoint handed on a round's last page covers the round, which this link does not. The way out is the owner's: take the link before the band pages and follow it at the round's end, accept the stale row, or have the standard let an unfiltered enumeration of the same round speak for a member an earlier page of it listed.
- The removes of a backend that cannot search its target (DAV, Gmail, Google) relocate without asking, as before, and say so.
- A relay whose append the server does not name still fails after the append succeeded.
- No server test for Y2's two cases; ./tests/pimdir.sh and the Stalwart, Radicale and live suites wait for the owner, after the stores are migrated.
- D2, Y4, P5, N1, N2 and an `Abandon` verb, out of this change.

## Capabilities moved

- sync (cairn/spec/sync.md): added "The owner's files live under local/", "A remove relocates or deletes, and says which", "An add its server does not name is accepted with no handle", "A create left waiting is reported", "A member named apart from its create is reported", "A create naming no member is owed no body fetch", "An intent stating an unknown version stays pending"; modified "An account holds named sources" (the drafts), "A store the format outgrew is refused with its remedy" (its scenario), "A run holds the store lock, waiting bounded", "A send that may have been delivered is never sent again", "The sidecar is replaced whole", "The sidecar holds each account", "A store laid out by an earlier neverest is refused", "A store error carries its code".
