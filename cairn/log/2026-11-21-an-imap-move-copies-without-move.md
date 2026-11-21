---
cairn: log
change: an-imap-move-copies-without-move
date: 2026-11-21
---

# An IMAP move copies without MOVE

Issue [#35](https://github.com/pimalaya/neverest/issues/35): OVH (Zimbra) advertises IMAP4rev1 and UIDPLUS but no MOVE, answered every `UID MOVE` with `BAD`, then closed the connection, so the rest of the run failed. PR [#36](https://github.com/pimalaya/neverest/pull/36) is not taken, for the reasons in the proposal.

## What landed

- src/imap/client.rs: `ImapClient::supports_move`, MOVE or the IMAP4rev2 atom, as `supports_uid_expunge` matches.
- src/imap/backend.rs: `move_messages` keeps its signature. It sends `UID MOVE` where the server takes it, else refuses before any command without `UID EXPUNGE`, else sends `UID COPY` then the `\Deleted` store and `UID EXPUNGE` of the source UIDs, through `expunge_uids`, which `delete_message` now shares.
- No search in the adapter: `targets_holding` (src/sync/remote.rs) already asks the target for a searchable identity before every move, so a move interrupted after its copy is finished by the next run with a delete. A member under a minted key moves the same way and may leave a second copy if interrupted, never none (owner, 2026-11-21).
- src/imap/fake.rs: the fake announces MOVE; `without_move()` withholds it and answers `UID MOVE` with `BAD`, as OVH does.

## Spec

[sync](../spec/sync.md) gains *An IMAP move without MOVE copies, then expunges its own messages*.

## Verified

The three driver tests `a_remove_relocating_without_move_copies_then_expunges`, `a_remove_under_a_minted_key_without_move_copies_unasked` and `a_remove_relocating_without_move_nor_uidplus_is_held_unwritten` failed on 207eba0 (rejected after `UID MOVE`) and pass. `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo test --locked --all-features` (654 unit tests, the maddy suite tests/without_uidplus.rs 2 passed) and the five builds of CONTRIBUTING.md. Not run against OVH: no account here, the reporter is asked on #35.
