---
cairn: tasks
change: gtd-hardening
---

# Tasks

Track E of unfog cairn/changes/gtd-hardening; uses io-pimdir cairn/changes/gtd-hardening (`PimdirChangeKind::Add::supersedes`, `PimdirSuperseded`) and io-webdav cairn/log/2026-10-29-gtd-hardening.md (`move_item`, collection ids by path, reconnect). Neither is pushed: every cargo command fetches them from the local checkouts (`CARGO_NET_GIT_FETCH_WITH_CLI=true` and a `url.file://...insteadOf` per crate).

- [x] Pin io-pimdir 8907303902a8d159ad40d27934de802c3afa7d24 and io-webdav b2b9a00462f7d371de3fb419e53288f0a13cee39 in `[patch.crates-io]`; Cargo.lock on their `git+https://github.com/pimalaya/...?rev=...` sources; API drift (`supersedes` on `PimdirChangeKind::Add`)
- [x] `DavClient::move_item` (`MOVE`, `None` on 405, 501, 404 and for CardDAV), `Client::move_item` (`None` off DAV)
- [x] `PimRemote::push`: an `Add` carrying `supersedes` moves then writes the body, falls back to the upload, rejects a refused move (a duplicate `UID` as a refused create); tests and regression tests
- [x] A DAV delete answered 404 or 410 is accepted; test and regression test
- [x] `DavClient::move_items` moves with `MOVE`, re-creating then deleting only where the server cannot; tests and regression tests
- [x] Calendar ids starting with a slash: the default calendar keyed as the listing keys it; addressing, hub id and filter tested with `/caldav/v2/x%40g/events`
- [x] fmt, clippy `--all-targets`, `cargo test`; CHANGELOG
- [x] Fold into cairn/spec/sync.md, log, status landed (2026-10-29)

## Notes

- Tests, all offline over a scripted DAV stream (src/dav/client.rs `scripted`, `DavClient::scripted`) or a scratch store: src/offline/remote.rs `push_tests` (`a_superseding_create_is_delivered_by_a_move_then_a_write`, `a_move_answering_no_etag_is_written_over_unconditionally`, `a_server_without_move_has_the_create_uploaded`, `a_move_refused_for_its_identity_rejects_the_create`, `a_remove_of_a_member_already_gone_is_accepted`, the target calendar being `/caldav/v2/team%40g/events`); src/dav/client.rs `an_item_moves_by_one_server_side_move`, `a_move_the_server_cannot_do_is_answered_none`, `a_card_is_never_moved_server_side`, `a_relocation_moves_rather_than_recreates`, `a_relocation_recreates_where_the_server_cannot_move`, `a_delete_of_a_member_already_gone_is_done`, `a_calendar_outside_the_home_set_is_addressed_by_its_path`, `a_default_calendar_outside_the_home_set_is_keyed_by_its_path`; src/offline/driver.rs `a_filter_names_a_calendar_by_its_path`.
- Regression proof (2026-10-29), on a scratch copy of the working tree before this change with the two pins applied, the tests and the test-only scripted stream grafted in: every test above fails but `a_card_is_never_moved_server_side` and `a_filter_names_a_calendar_by_its_path`, which pass (the first calls the new seam through its shim, the second documents a path that already held). Failures: the old push uploads (`PUT` alone where `MOVE` then `PUT` is expected; a 412 rejected as `append`, not `move`); a 404 delete is `Rejected`; `move_items` reads, creates and deletes; the default calendar is keyed `events`. Shims holding the old behaviour where a seam did not exist: `DavClient::move_item` answering `None` (the old session had no move), `collection_key(home, path)` reading the last segment as the old one did; compile fix `supersedes: _` in the old push's pattern; `#[derive(Debug)]` on `WrittenItem` for the tests' unwraps.
- io-webdav quotes the ETag it sends in `If-Match` and strips the quotes from the one it reads, so the revisions neverest stores and passes stay bare.
- io-webdav's client from `connect` now resends once a request the server closed the connection before answering; `DavClient`'s own reopen (`A DAV connection survives a server that closes it`) stays as the fallback should that resend meet a closed stream too.
- Provisional (move then write): a write failing after the move rejects the add with the reason naming the moved handle, the member left in the target under its old body and the create pending; the target's next listing names it, which lands the create by its arrival (pimdir SYNC §6) and merges the staged body against the server's. Accepting it with no revision instead would read the old body as a remote edit and lose the staged one.
- Provisional (origin): an `Add` carrying both an origin and a superseded member tries the move first, then the copy or upload it would have made without one.
- Provisional (relocation): `move_items` sends no `If-Match` (its `Client` signature carries none, IMAP and Graph sharing it); `Overwrite: F` still keeps it off a member already at the target, and a move carries the body whatever its ETag.
- Provisional (other backends): only DAV accepts a delete of a member already gone here; the superseded tombstone's delete arises only where a move delivered the create, which only CalDAV does.
- Provisional (ids): a store synced before this change holds a Google calendar as `caldav/events` (whichever one the listing kept); its calendars now list under their paths and sync as new collections, `caldav/events` remaining while the home-set primary is still listed as `events`. Nothing migrates the old rows.
- Open (paired create by path): a pair creating, on the other endpoint, a calendar the first one keys by a path (`/caldav/v2/x%40g/events` from Google) sends `MKCALENDAR` at that path on the other server, where it means nothing; the pair keys both endpoints by one wire id, so the created one must be reachable under the same id. Out of this change; it needs a choice of id mapping across endpoints.
