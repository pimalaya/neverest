---
cairn: tasks
change: a-flag-push-names-what-it-changes
---

# Tasks

- [x] `client`: `update_flags` with an add and a remove set; `offline/remote` pushes `SetFlags` through it.
- [x] `imap`: `+FLAGS` and `-FLAGS` stores; the fake server applies `UID STORE`.
- [x] `msgraph`: a patch of the touched properties only; nothing sent for a change naming none.
- [x] `gmail`: one modify of the touched labels only.
- [x] `dav`, `gcal`, `gpeople`: `update_flags` refuses, as `store_flags` did.
- [x] Tests, failing first: per connector, a flag another client changed after the listing survives a push removing another.
- [x] Spec folded, log written, CHANGELOG.
