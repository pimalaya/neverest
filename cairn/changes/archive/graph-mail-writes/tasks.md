---
cairn: tasks
change: graph-mail-writes
---

# Tasks

- [x] `GraphClient::move_messages`, `copy_message`, and `add_message` (Drafts only, flags patched after).
- [x] `Client::copy_item`, and `PimRemote::push` delivering an `Add` with an origin by copy where the backend copies.
- [x] Declarations: move and copy full, add and flags.draft partial; unit test.
- [x] Live tests (tests/msgraph.rs): a message bounced four times between the Inbox and a throwaway folder then moved to Deleted Items, held once on the server and in the store under one `seq` after every run (both halves delivered: relocations out of the Inbox, copies into it); a draft added to Drafts lands once, read, and an add elsewhere is rejected and files nothing.
- [x] Spec, CHANGELOG, log entry.
