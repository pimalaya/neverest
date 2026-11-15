---
cairn: tasks
change: a-check-lists-collections-with-their-roles
---

# Tasks

- [x] `client`: `collection_roles`, empty for the backends without roles.
- [x] IMAP: roles from the `LIST` attributes and `INBOX`; unit test on LIST rows.
- [x] Graph mail: roles from the well-known folders; unit test on the id match.
- [x] Gmail API: roles from the system labels.
- [x] `cli/check`: `collections: [{ id, name, role? }]`, display unchanged.
- [x] `tests/submit.rs`: the Stalwart check lists INBOX and the SPECIAL-USE folders with their roles.
- [x] `tests/msgraph.rs` live: the check names inbox, drafts, sent and trash.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`.
