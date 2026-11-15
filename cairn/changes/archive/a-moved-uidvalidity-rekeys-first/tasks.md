---
cairn: tasks
change: a-moved-uidvalidity-rekeys-first
---

# Tasks

- [x] `client`: `Listed::Rekey`; `PimRemote::enumerate` records it and fails the enumeration.
- [x] `imap`: the bound `UIDVALIDITY` (stored checkpoint, else the open round's) checked on every listing path: QRESYNC delta, delta without QRESYNC, delta head, band head, round first page, resumed round page.
- [x] `offline/remote`: `Bound` takes the open round's checkpoint when no round closed.
- [x] `offline/driver`: a sync naming a rekey runs `PimdirRekey` then syncs again; the post-sync epoch guard and `Client::handle_space_epoch` removed.
- [x] Tests, failing first: a fake IMAP server whose `UIDVALIDITY` moved answers `Listed::Rekey` on every path; a sync over it rebinds the message by link id, drops nothing `Deleted` and carries a pending flag to the new UID.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
