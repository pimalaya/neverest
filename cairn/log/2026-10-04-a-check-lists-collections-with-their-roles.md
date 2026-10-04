---
cairn: log
change: a-check-lists-collections-with-their-roles
date: 2026-10-04
---

# A check lists collections with their roles

`neverest check` reported how many collections each source listed, not which, nor what each is for. MOA, adding a mail account, asked himalaya for the folders' uses instead (IMAP SPECIAL-USE, Graph's well-known folders), the only reason its shipped himalaya kept the `imap` and `msgraph` features.

## What landed

- src/client.rs: `Client::collection_roles`, a check-only probe answering collection id → role; empty for DAV, Google Calendar and People.
- src/imap/backend.rs: `mailbox_roles`, from the `LIST` rows' SPECIAL-USE attributes (`\Sent`, `\Drafts`, `\Trash`, `\Junk`, `\All`, `\Archive`) and `INBOX`.
- src/msgraph/client.rs: `mailbox_roles`, one well-known folder lookup per role (`inbox`, `sentitems`, `drafts`, `deleteditems`, `junkemail`, `archive`), matched to the listing by id; a missing one is skipped. Mail only.
- src/gmail/client.rs: `mailbox_roles`, from the system labels.
- src/cli/check.rs: each source reports `collections: [{ id, name, role? }]` instead of a count; the text output is unchanged.
- Tests: unit tests per backend; `tests/submit.rs` `check_lists_collections_with_the_roles_the_server_states` against Stalwart; `tests/msgraph.rs` `a_graph_check_names_the_well_known_folders`, run live on the test tenant.

## Capabilities moved

- sync: a check lists collections with their roles.
