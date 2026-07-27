---
cairn: tasks
change: select-once-per-connection
---

- [x] `ImapClient` gains a `selected: Option<String>` cache with `mark_selected`
      / `is_selected`.
- [x] `enumerate`'s plain and QRESYNC selects record the selection.
- [x] `select_cached(mailbox)` helper on the backend; all fire-and-forget selects
      (meta/size fetch, body fetch, store, move, delete, append UID recovery)
      route through it.
- [x] Build, fmt, clippy clean; 14 unit tests + relay integration unregressed.
- [x] Live Stalwart: bodies still correct; SELECTs drop to ~one per connection.
- [ ] Fold delta into `cairn/spec/sync.md`; write log entry.
