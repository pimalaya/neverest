---
cairn: log
change: an-append-is-searched-by-its-whole-message-id
date: 2026-11-20
---

# An append is searched by its whole Message-ID

Found writing a-server-without-uidplus-runs-in-ci: on a server without UIDPLUS, `add_message_stream` names the message it appended by `UID SEARCH HEADER Message-ID` on the bare id and takes the highest UID. IMAP `HEADER` matches substrings, so a message stating `<b@x.y>` that another client delivered after the append of `<b@x>` (a higher UID) was taken for it, binding the create to a message it is not. `find_message`, which asks a move's target, already searched the bracketed id.

## What landed

- src/imap/backend.rs: `add_message_stream` searches `<message_id>`, as `find_message` does; it still takes the highest UID, the newest copy being the one appended.
- src/imap/fake.rs: `FakeImap::deliver_after_next_append` has another client deliver a message right after the next kept `APPEND`, before any search.
- src/imap/backend.rs: `an_append_without_uidplus_is_named_by_its_whole_message_id`, failing on the code before (it answered UID 9, the delivered `<b@x.y>`, for the append at UID 8).

## Spec

- [sync](../spec/sync.md): *An add its server does not name is accepted with no handle* states the bracketed search and the highest UID, scenario "An append searched for beside a longer Message-ID".

## Verified

- `cargo test --locked --all-features` in the dev shell, tests/without_uidplus.rs against maddy included: all pass. `cargo clippy --locked --all-targets -D warnings` clean over all features, the default set and rustls-ring+imap; `cargo fmt --check` clean.
