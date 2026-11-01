---
cairn: log
change: neverest-follows-sync-draft-05
date: 2026-10-30
---

# neverest follows sync-draft-05

Phase A part 2a of the pimdir stable-release plan: the rest of the port to io-pimdir's storage-draft-05 store and sync-draft-05 engine (local checkout, path pin unchanged), beside a-flag-push-names-what-it-changes, a-mass-absence-waits-for-a-person and the-engine-pins-the-agreed-body.

## What landed

- src/conflict.rs, src/offline/driver.rs (`merge_conflicts`, `keep_store_bodies`): `if_match` on the queued `update` (the local side a resolution was decided on, the local side a merge merged) and on the `Edit` mutation (the body a one-way overwrite restates); bodies staged with their action through `enqueue_with_body`.
- src/offline/mod.rs, src/offline/prof.rs: the runner answers `PimdirYield::WantsVerify` through the store, timed as `verify`.
- src/offline/driver.rs, src/sync/report.rs: `unreadable` (changes withheld over a body missing or not matching its name, exit 2) and `overruled` (a body a policy did not adopt, kept as a retained item, a warning), noted per side and drained by `itemize_noted`.
- src/kind/mod.rs: `split_link_id` names a stated identity filed with `esc:` without it, minted or not, as `PimdirLinkId::stated` does; a derived key still names none.
- src/offline/remote.rs: a stored body streams through io-pimdir's verifying `PimdirBlobReader`.
- src/offline/driver.rs: `bind_relayed` reads `write`'s `PimdirWritten`.
- tests/drain.rs: gated on `dav`; the card conflict tests of src/conflict.rs and src/cli/conflict.rs on a contacts backend, so a mail-only test build passes (they failed there with `This build cannot settle items of type text/vcard`). Fixtures name bodies by their real hash (`old_hash`, `new_hash`, the rekey and pull-plan tests); the test writers stage through `enqueue_with_body`, tests/multikind.rs stating the body it read.
- Tests: `an_escaped_key_names_its_stated_identity`, `an_unreadable_body_waits_and_an_overruled_one_does_not`.

## Evidence

On the old code `an_escaped_key_names_its_stated_identity` named `esc:alt:a@example.org` to the server. `a_local_body_edit_is_pushed_conditionally_and_confirmed` failed against the new engine with no push (`expected one If-Match update, got []`): its body was named `ed17`, so the verification withheld it; the fixture now names it by its hash.

## Not done

- The search indexer (neverest-builds-the-search-index) waits for io-pimdir's search API.

## Capabilities moved

- sync: an edit names the body it was made over, a change over an unreadable body is withheld and reported, a body a policy overruled is kept and reported, a stated identity is named without its escape (added); a body reaches the blob tree under a lock (modified: `enqueue_with_body`).

## Verified

`cargo fmt`, `cargo clippy --locked --all-features --all-targets -D warnings` and `cargo test --locked --all-features` (465 passed, 67 ignored: the Stalwart, Radicale and live suites), and clippy and tests with `--no-default-features` and `rustls-ring` alone, with `imap`, `imap,smtp`, `dav`, `msgraph`, `gmail`, `gcal`, `gpeople` each, and MOA's set.
