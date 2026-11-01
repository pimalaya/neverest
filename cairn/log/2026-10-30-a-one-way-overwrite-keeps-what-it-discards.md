---
cairn: log
change: a-one-way-overwrite-keeps-what-it-discards
date: 2026-10-30
---

# A one-way overwrite keeps what it discards

The port to io-pimdir's an-automatic-settle-keeps-what-it-discards (pimdir SYNC §7, §9, STORAGE §11; path pin ../io-pimdir unchanged): an `Edit` stating `overrule` keeps each diverging body it does not adopt as a retained item, reported `Overruled`, and is refused (`conflict-unread`) while the conflict holds a body nobody read.

## What landed

- src/offline/driver.rs: `keep_store_bodies` states `overrule: true` and takes the side's remote; it first fetches (upgrades, `PimdirTier::Full`) every conflicted placement holding no diverging body, as `resolve_conflicts` does for a merge (`fetch_diverging`), and an edit still refused `conflict-unread` fetches its handle once and retries; one unread after its fetch stays parked (a conflict the run reports), never failing the run. Another store refusal carries its code (`pimdir_error`).
- A person's `conflict resolve` stages through the queue (`enqueue_with_body_checked`) and builds no `Edit`, so it overrules nothing: a person's choice may discard. No exhaustive `match` on `PimdirWriteOp` exists in neverest, so `PimdirWriteOp::Overrule` needed no arm.
- src/error.rs: `conflict-unread` is left out of the relayed codes, beside the other codes neverest cannot meet: the one-way overwrite answers it with a fetch and never raises it.
- Tests: `a_one_way_overwrite_keeps_the_target_body_as_a_retained_item` un-ignored; `a_one_way_overwrite_fetches_an_unread_target_body_before_keeping_it` (the target lists its edit with no body: fetched once, then kept and noted under `overruled`); src/offline/index.rs `an_index_of_another_shape_is_rebuilt_by_the_next_refresh`.
- `neverest check` opens no store (src/cli/check.rs reads only the endpoints), so there was no writability probe to move to `PimdirProducer::probe`.
- CHANGELOG: the one-way bullet states again that the target's edit is kept as a retained item and reported under `overruled`.

## Evidence

- With `overrule: false` on the new io-pimdir, `a_one_way_overwrite_keeps_the_target_body_as_a_retained_item` found no retained item (`left: 0, right: 1`), as on the old io-pimdir.
- With `fetch_diverging` disabled, `a_one_way_overwrite_fetches_an_unread_target_body_before_keeping_it` kept nothing (`left: 0, right: 1`): the store refused the overrule and the item stayed parked.

## The index rebuild

io-pimdir's the-index-reads-the-canonical-columns changed the index shape: the indexer's open rebuilds an index of another shape, which `after_run` and `neverest index` both go through. A rebuild reads as a whole fold (`folded` counting every change again), not as an error; io-pimdir exposes no rebuild flag, so the report names none. Checked on a scratch store: `neverest index --json` answered `{"folded":3,"indexed":3,"behind":0,"busy":false}`, exit 0; after `DROP INDEX placement_by_link` on index.db, it rebuilt and answered the same.

## Not covered, open

A sync's `PreferLocal` on a one-way pushing pass can still push the source's body over a target edit arriving between the pull pass and the push pass, keeping nothing (pimdir SYNC §5, vector 15): left for its own change, pending a decision.

## Checks

`cargo fmt`; `cargo clippy --locked --all-features --all-targets -- -D warnings`; `cargo test --locked --all-features` (464 unit tests, none ignored); clippy and tests with `--no-default-features` and `rustls-ring` alone, then with `imap`, `imap,smtp`, `dav`, `msgraph`, `gmail`, `gcal`, `gpeople` each.

## Capabilities moved

- sync: a one-way overwrite keeps the body it discards (added).
