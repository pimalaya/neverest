---
cairn: tasks
change: an-imap-move-copies-without-move
---

# Tasks

- [x] Owner validates the proposal and rules on the open decision (minted keys move too, 2026-11-21).
- [x] Work apart from the uncommitted a-member-listed-with-an-error-is-not-removed work (CHANGELOG.md, cairn/spec/sync.md, src/imap/backend.rs, src/sync/remote.rs): done in a clone of master, so this change commits alone.
- [x] Fake: src/imap/fake.rs announces MOVE by default; a `without_move()` switch withholds it and answers `UID MOVE` with `BAD`, as OVH does.
- [x] Tests first, failing on master, in src/sync/driver/tests.rs through `pushed_to_fake_imap`: without MOVE, a move sends `UID COPY 7 Archive`, `UID STORE 7 +FLAGS (\Deleted)`, `UID EXPUNGE 7` and no `UID MOVE`, and is relocated; the same under a minted link id; without MOVE nor UIDPLUS, it is rejected with nothing written. A `supports_move` unit test beside the `supports_uid_expunge` one in src/imap/client.rs.
- [x] Code: `ImapClient::supports_move` in src/imap/client.rs; `move_messages` in src/imap/backend.rs, the STORE and `UID EXPUNGE` shared with `delete_message` through `expunge_uids`. No signature change.
- [x] Checks: `cargo test --all-features` (maddy suite included), the five builds of CONTRIBUTING.md, clippy with `-D warnings`, `cargo fmt --check`.
- [x] Fold the delta into cairn/spec/sync.md after "An IMAP delete expunges its own message alone", write the log, CHANGELOG `[Unreleased]` Fixed entry naming #35, status `landed`.
- [x] Commit: `fix(imap): copy then expunge a move when the server lacks MOVE`.
- [ ] Push, owner's go first.
- [ ] Owner posts the two comments of proposal.md: closes #36, asks the reporter on #35 to try master on OVH; #35 closes on their confirmation.
