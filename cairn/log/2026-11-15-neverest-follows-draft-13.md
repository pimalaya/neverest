---
cairn: log
change: neverest-follows-draft-13
date: 2026-11-15
---

# neverest follows draft-13

On io-pimdir 0.14.0 from crates.io (d184622 plus its prepare commit 77890ad; pimdir 1c6442e: storage-draft-13, sync-draft-13, search-draft-13), first pinned by git at d184622 and moved to the release once it was published, no `[patch]` left. neverest goes from draft-11 straight to draft-13, so every store is rebuilt once. The same pass took the X14 releases: pimalaya-cli 0.6.0 (its `Listing` is not used here, no output moved), io-imap 0.11.2, io-gpeople 0.8.1, io-msgraph 0.9.2, io-gcal 0.5.1, the Cargo.lock diff kept to the six crates. The tree built on the pin as it stood: neverest matches no `PimdirPushOutcome`, and its `PimdirSyncEvent` matches end in `_`. Two unit tests failed on the pin before any change: the store-version test (draft-12 had been the newer one) and Graph's band test (a band member deleted before the round closed was no longer found absent).

## What landed

- **The narrowed-round rule (N27).** A narrowed round handing no `enumerated` finds no member absent (pimdir SYNC §5), so every band round now hands it on its last page: src/imap/backend.rs (`narrowed && next.is_none()`, one `UID SEARCH ALL`), src/gmail/client.rs (`narrowed && page.next_page_token.is_none()`, the label's ids with no query) and src/msgraph/client/mail.rs (`folder_ids`, a fresh delta with the pass's `$select`, its link dropped, a band keeping the stored link).
- **Refusals as they stand (N30).** src/error.rs `refusal` reads a chain as a refusal: a coded `unsupported` (Graph's append outside Drafts, an override Google or Graph takes through no instance, stated before any request) or a protocol's `refuses`: src/dav/client.rs, src/gmail/client.rs, src/gcal/client.rs, src/gpeople/client.rs and src/msgraph/client.rs (mail, contacts and calendars) by `names_the_request` (400 to 499 but 401, 407, 408, 409, 412, 423, 425, 429), src/imap/backend.rs by a `NO` naming `CANNOT`, `PARSE`, `TOOBIG` or `NOPERM`. A quota or a rate limit is never one. src/offline/remote.rs `fail` answers `Refused { reason }` on every server write (set-flags, move, append, copy, delete, update, a superseding move), `Rejected` otherwise; a body missing from the blob tree and a copy's identity search stay rejections.
- **The output.** src/offline/driver.rs `Noted` keeps each `Refused` event once a run, keyed by collection and handle; `itemize_noted` reports it under `refusals` (src/sync/report.rs `RefusedChange`: `side`, `collection`, `id` the key of a create, `reason`), takes the hunk back, and counts a warning (exit 3). src/cli/sync.rs prints a `refused` event per row once the account ends (src/sync/event.rs `SyncEvent::Refused`).
- **The store refusal** names the rebuild: "move the store directory away, then rebuild it with `neverest init` and `neverest sync`".
- **The pending-create withdrawal** is io-pimdir's gate; neverest only states it in the spec (Graph mail).

## Tests

Seen failing on the code before the change (each production edit reverted by hand for one run, never through git):

- src/imap/backend.rs `a_narrowed_round_hands_every_uid_on_its_last_page` (the band case now hands `["1", "2"]`) and src/gmail/client.rs `a_band_round_hands_the_label_s_ids_on_its_last_page` (new): both failed with the `!band` condition back.
- src/msgraph/client/mail.rs `a_band_round_lists_its_head_and_its_last_page_keeps_it`: failed on the pin before `folder_ids` (the deleted band member kept); `widenings_list_their_band_and_keep_the_link`, `a_whole_folder_link_serves_every_scope_after_it` and `a_scoped_round_is_narrowed_and_its_pass_enumerates_the_folder` count the band's delta pass now.
- src/offline/driver.rs `a_create_refused_as_it_stands_is_held_and_reported` (new): over the fake IMAP server, a create answered `NO [CANNOT]` is appended once, held by the second run, its refusal standing in the store (`PimdirReader::refusals`), reported once under `refusals`, its hunk taken back, exit 3; one answered `NO [OVERQUOTA]` is appended again and reported under `rejected`. With `fail` answering `Rejected` it failed: "held, never appended again, left: 2".
- New mapping tests, each through the connector's own write path over a scripted server: src/imap/backend.rs `a_write_refused_as_it_stands_is_told_from_one_that_may_land`, src/gmail/client.rs and src/msgraph/client.rs `a_write_refused_as_it_stands_is_told_by_its_status` (Graph mail, contacts and calendar), src/gcal/client.rs `an_event_refused_as_it_stands_is_told_by_its_status` (a floating-time 400 against a 409 and a 429), src/gpeople/client.rs `a_write_refused_as_it_stands_is_told_by_its_status`, src/offline/remote.rs `a_create_refused_as_it_stands_is_refused_not_rejected` (a DAV PUT: 400 and 403 `valid-calendar-data` refused, 409, 412, a 403 quota and a 507 rejected), src/msgraph/client.rs `an_add_outside_drafts_is_unsupported` (now refused too), src/error.rs `a_status_names_the_request_but_a_few` and `an_unsupported_change_is_refused_and_a_quota_is_not`, src/sync/event.rs `a_refusal_is_an_event_named_as_its_row`.
- Draft names moved: src/offline/driver.rs `a_store_version_tells_an_earlier_neverest_from_a_newer_one` (draft-11 and draft-12 earlier, draft-14 newer), tests/drain.rs `a_store_an_earlier_neverest_wrote_asks_for_a_reinstall` (restamps `storage-draft-11`, asserts the rebuild), src/offline/index.rs `an_index_of_an_earlier_draft_is_rebuilt` (`search-draft-11`).
- io-pimdir's own vector runner (tests/vectors_sync.rs at 77890ad, format 7, 167 cases, 162 to 167 among them) passes; neverest has no sync vector runner of its own, its engine being io-pimdir's.

## Evidence

Each as `nix develop --command <cmd>` with `CARGO_TARGET_DIR=target/alignment`, on 2026-11-15: `cargo fmt --check` clean; `cargo test --locked --all-features` all passing (598 unit tests, 645 tests in all); `cargo test --locked --no-default-features --features rustls-ring,imap,smtp` passing; `cargo clippy --locked --all-targets -D warnings` over the 12 sets of .github/workflows/tests.yml clean; `cargo deny check` ok; `sh cairn/verify.sh` (the untracked cairn/spec/testing/ and cairn/changes/jmap-sources/ left as they are). The server and live suites were not run.

## Not done

- **A create offered to a source binding nothing** (propagated from another source) has no binding to record its refusal on (pimdir SYNC §5, "Left open" in pimdir's log): it is pushed and reported under `refusals` by every run. io-pimdir's to settle.
- **The queue receipts' refusals** (`PimdirActionStatus::Applied::refusals`, `PimdirReader::receipt_refusals`) are not printed by `neverest drain`, which applies rows before any sync could refuse them.
- **A transport failure** stays `Rejected`, as before, rather than unreported: a connector that cannot tell answers `Rejected`.

## Capabilities moved

- sync (cairn/spec/sync.md): added "A change refused as it stands is held and reported"; modified the header, "A store the format outgrew is refused with its remedy", "A refused write is reported, never counted as applied", "A sync prints its progress as events", "Graph mail moves, copies and adds drafts", "A mail sync lists within a scope on the `Date`", "A provider narrows a scope with a margin", "A Graph mail folder keeps one delta link over the whole folder", "A capped source says so, and keeps what it leaves unlisted".
