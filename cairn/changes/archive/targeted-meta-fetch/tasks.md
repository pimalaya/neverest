---
cairn: tasks
change: targeted-meta-fetch
---

- [x] Add `fetch_envelopes(mailbox, uids)` to the IMAP backend (`UID FETCH <set>
      (UID FLAGS ENVELOPE RFC822.SIZE)`), reusing `build_item_names`/`envelope_from`.
- [x] Add `fetch_sizes(mailbox, uids)` to the IMAP backend (`UID FETCH <set>
      (UID RFC822.SIZE)`), returning `(uid, size)` pairs.
- [x] Expose both on `Client` (imap arm).
- [x] `EmailRemote::fetch_meta` targets the requested handles via `fetch_envelopes`.
- [x] `EmailRemote::sizes` targets the requested handles via `fetch_sizes`.
- [x] `cargo fmt` + `clippy` clean; unit tests pass; relay integration unregressed.
- [x] Fold delta into `cairn/spec/sync.md`; write log entry.
