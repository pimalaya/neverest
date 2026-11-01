---
cairn: log
change: neverest-follows-sync-draft-07
date: 2026-11-01
---

# neverest follows storage-draft-08 and sync-draft-07

The port to io-pimdir bff5737 (pimdir storage-draft-08, sync-draft-07, search-draft-08), still through the `../io-pimdir` path patch: the pin to the git rev, the Stalwart and live suites and the commit are left (see Not done).

## What landed

- Cargo.toml, Cargo.lock: `rusqlite = "0.41"`; the lock moves io-pimdir's dependency list, rusqlite (0.41.1) and libsqlite3-sys (0.39.1) alone.
- The compile port: `PimdirLoadSelection` (src/offline/storage.rs, src/offline/driver.rs, src/msgraph/client/mail.rs); `CALENDAR_COMPONENT_VTODO` (src/offline/capability.rs, src/offline/driver.rs, src/cli/check.rs, whose test read the row by the literal `x-vtodo`); `PimdirSubmit` without `object` and with a required `source` (src/offline/submit.rs, src/offline/create.rs, tests/submit.rs, tests/intents.rs); `narrowed` and `enumerated` on the snapshot (src/offline/remote.rs); `PimdirError::Version` for `Stale`; `hash::hash` for `PimdirHashAlgo`; `PimdirBlobs::open(dir)` and `path` answering `io::Result`. The second pass showed no further error: neverest matches the `#[non_exhaustive]` enums with wildcards already, calls no `PimdirParked::guard`, and copies none of the types that lost `Copy`.
- Narrowed rounds (src/client.rs `Enumeration::narrowed`, `enumerated`; src/offline/remote.rs `Narrowing`): IMAP (src/imap/backend.rs) states `narrowed` when `SENTSINCE` or `SENTBEFORE` narrows the round and hands `UID SEARCH ALL` on its last page; Gmail (src/gmail/client.rs) when the round has a query, handing the label's ids listed with no query; Graph (src/msgraph/client/mail.rs) on every band page and the pass, the pass handing the ids it read. A band round (the band a coverage lacks) states `narrowed` and hands no enumeration on all three: the engine finds absent there only a member dated in the band, below the round's opening, never an undated one (io-pimdir src/sync.rs `RoundPlan::finds_absent`, `stamped`), so an enumeration there would cost a listing and keep nothing.
- Upgrades (src/offline/driver.rs): `SourceCtx::sync_options` and `upgrade_options` (`PimdirUpgradeOptions::from(&sync_options)`, pushing as the endpoint's pushing sync does); every upgrade takes them (`apply_batch`, `resolve_conflicts`, `fetch_diverging` and `keep_store_bodies`, `hydrate_copies`, `hydrate_full_collection`), and each upgrade's `Overruled` events reach `overruled` through `Noted::overrule`, the landings of phase 2 and a full hydration drained after them.
- Report (src/sync/report.rs, src/cli/sync.rs): `absences` and `--accept-absences` name the empty-listing park. `overruled`, `unanswered`, `unreadable`, `rejected_creates` and the drain's `overruled` were reported already by the 2026-10-30 work; the exit code is unchanged.
- Errors (src/error.rs): `store-version`, `sync-version`, `store-exposed`, `action-malformed`, `flag-invalid` and `index-version` relayed; `store-newer`, `store-older` and `store-stale` gone; `index-zone-unknown` and `conflict-unread` left out with their reason. `open_store` tells an earlier neverest's store from a newer one's (`written_earlier`: no version or a lower `storage-draft-NN`) and names the reinstall, never a delete; `init` on an initialized account names `sync --reset` where it said to delete the store.
- Modes: io-pimdir restricts the directory it creates a store in, so `init` needed no change (tested under umask 022). The dry-run replica (`DryRunReplica::new`, `clone_dir`) and the directory `sync --reset --force` moves a store into are created through `io_pimdir::client::modes`: under the umask both were refused as `store-exposed`, so no dry run ran and the moved store could not be read back.
- Intents: an intent whose payload does not decode names no source and still goes to a side of its namespace, which parks it as malformed (comments in src/offline/driver.rs).
- Docs: cairn/spec/sync.md (the two scope requirements rewritten, the store version, upgrade, error code, absence, one-way overwrite, reset and index requirements, two requirements added, the drafts named in the header, `calendar.component.vtodo`), CHANGELOG.md, MIGRATION.md (the reinstall order).

## Changed assertions

- src/offline/driver.rs `a_store_a_newer_neverest_wrote_asks_for_an_upgrade` and tests/drain.rs `a_store_a_newer_neverest_wrote_fails_with_its_code`: a later draft is stamped in `store_meta.format_version` (a `user_version` of 99 now reads as `store-corrupt`), the code is `store-version`.
- tests/drain.rs `a_database_another_program_wrote_fails_as_foreign`: the foreign database is made `0600`, else it is refused as `store-exposed` first.
- src/offline/driver.rs `a_server_delete_is_named_once_on_each_endpoint`: a listing naming no card now parks; the test asserts the park, then accepts the absences.
- src/offline/driver.rs `a_drain_applies_an_action_as_the_source_of_its_collection`, `a_send_needs_its_source_opened_only_when_it_sends_natively`, `a_submit_intent_survives_the_drain_and_is_read_back`: the queued payloads state what the schemas require (an `add` its body, a `submit` its `source` and no `object`), the last reading the row's body.
- src/offline/submit.rs `a_payload_naming_another_body_parks` became `a_payload_naming_a_body_parks`, and `a_payload_reads_alike_whatever_its_key_order` drops `object` from its payloads.
- Fixtures naming bodies by short fake hashes (src/offline/driver.rs `the_sweep_collects_the_bodies_the_purge_released`, `FullListingRemote`, src/offline/storage.rs `a_one_sided_body_projects_as_a_copy_and_is_a_hydration_target`) name them by their BLAKE3 hash: io-pimdir refuses a malformed name.
- The `keep_store_bodies` tests pass the one-way target's options (`STORE_WINS`); their assertions are unchanged, a divergence the fetch settled counting as kept.

## Evidence

- `a_scoped_round_keeps_what_its_arrival_filter_misses` (src/offline/driver.rs, Gmail over scripted sessions) with the Gmail round stating `narrowed = false`: the store held `["recent"]` where `["ahead", "draft", "recent"]` was expected, the undated draft and the message dated 2030 dropped.
- `a_narrowed_round_hands_every_uid_on_its_last_page` (src/imap/backend.rs) and `a_scoped_round_is_narrowed_and_its_pass_enumerates_the_folder` (src/msgraph/client/mail.rs) assert what the page states, false and none before.
- `a_fetched_divergence_settles_by_the_side_s_policy` runs the fetch under the target's options and under `PimdirUpgradeOptions::default()`, which every upgrade ran under before: the first settles (retained body, `Overruled`, the overwrite owed), the second leaves the item parked.
- With `fs::create_dir_all` and `fs::create_dir` put back, `a_dry_run_replica_opens_as_a_store` and `a_forced_reset_moves_the_store_aside` failed with `Exposed(...)`.
- `init_makes_a_store_its_readers_open` (tests/drain.rs, `umask 022` before `neverest init`, a fake CalDAV home set) passed before and after: it confirms io-pimdir's restriction rather than a change of neverest's.

## Gmail and Graph enumeration cost

- IMAP: one `UID SEARCH ALL` on the last page of a scoped round.
- Gmail: `messages.list` with the label and no query, 500 ids a request, 5 quota units each, once a scoped round: a label of 50,000 messages costs 100 requests and 500 units, against Gmail's 6,000 units a minute neverest paces to. Not measured on the test account yet.
- Graph: nothing more. The pass closing a scoped round already reads the folder's delta with ids, dates and flags; its ids are handed as they are.
- No band round enumerates (above).

## Not done

- The live suites (google.rs, msgraph.rs) and the Gmail cost on the test account: the supervisor runs them.
- The commit, and the change's landing.

## Supervisor follow-up (2026-11-01)

- Pinned to io-pimdir bff5737 by git rev; `cargo test --all-features` 489 passed on the pin.
- The Stalwart and Radicale suites ran from a scratch copy with the hard-coded ports shifted (rootless Docker cannot bind 143 and 144): 19 targets, 48 tests, all passing after two test fixes. tests/submit.rs `a_queued_submit_intent_leaves_through_smtp_and_comes_back_through_imap` queued its `submit` at `INBOX`, no collection of the store, which the draft-08 drain's capability backstop parks once the intent names its source; it now anchors at `imap/INBOX`. tests/create.rs `a_queued_calendar_is_created_on_the_caldav_server` asserted exit 0 on the run that parks the nested calendar, which waits for a person (exit 2) since the 2026-10-30 work; it now asserts exit 2.
- An end to end over a scratch store per account (Stalwart IMAP, Radicale CardDAV and CalDAV) with himalaya, cardamum and calendula queuing writes and neverest pushing them: every write reached its server.
- Live suites on the test accounts: Google 6 of 6 and Microsoft 13 of 13, after test fixes for two committed rules the suites had not met. A parked intent waits for a person (exit 2, the 2026-10-30 work): `sync_parked_intents` expects it and cancels the parked row as the person does (google.rs and msgraph.rs, the organizer's refused reply). A round listing a collection empty parks the absence (pimdir SYNC §5, this change): Graph calendars sync by rounds, so the step removing a throwaway calendar's last member confirms with `--accept-absences` (`sync_until_emptied` in `round_trip`, `sync_intents_emptied` for the two cancelled meetings); gcal syncs by delta and never parked.

## Capabilities moved

- sync: a narrowed round states it and enumerates (modified: "A mail sync lists within a scope on the `Date`", "A provider narrows a scope with a margin"); a fetched divergence settles by the endpoint's policy and a store is its user's alone (added); the store version, the newer store, the error codes, the empty-listing park, the one-way overwrite, the reset and the index requirements (modified); `calendar.component.vtodo` (renamed).

## Verified

`cargo fmt`; `cargo clippy --locked --all-features --all-targets -- -D warnings` clean, and with `--no-default-features --features rustls-ring,vendored` alone and with each of `imap`, `imap,smtp`, `dav`, `msgraph`, `gpeople`, `gcal`, `gmail`; `cargo check --all-targets` with `wizard` and `search` alone too, and `rustls-ring,imap` without `vendored`. `cargo test --locked --all-features`: 489 passed, 0 failed, 67 ignored (the Stalwart, Radicale and live suites); the default features the same. Per backend (`rustls-ring,vendored,<backend>`): imap 242, imap,smtp 262, dav 275, msgraph 302, gmail 278, gcal 239, gpeople 233 passed, none failed. The Radicale suites (tests/radicale.sh; calendars, merger, carddav, caldav, endpoints, `-- --ignored --test-threads=1`, the `pimdir` CLI built from ../io-pimdir into a scratch target): 17 passed.
