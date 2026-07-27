---
cairn: tasks
change: qresync-enumeration
---

# Tasks

- [x] `imap/client.rs`: keep capabilities; ENABLE QRESYNC/CONDSTORE on connect;
      `supports_qresync`; `select_delta` (QRESYNC SELECT).
- [x] `imap/backend.rs`: `enumerate(mailbox, cursor)` — QRESYNC delta when the
      cursor matches, else full `FETCH 1:* (UID FLAGS)`; `uid_flag_names` +
      `enum_entry` helpers (UID+FLAGS only, no ENVELOPE).
- [x] `client.rs`: neutral `Enumeration`/`EnumEntry` + `Client::enumerate`.
- [x] `offline/remote.rs`: `EmailRemote::enumerate` decodes the cursor, returns a
      delta/full snapshot, encodes `(UIDVALIDITY, HIGHESTMODSEQ)`; codec helpers.
- [x] Unit test: cursor round-trip / garbage / modseq-0.
- [x] Verify on two Stalwart servers: no-change sync = QRESYNC SELECT + 0 FETCH;
      new message picked up by the delta. Build/test/fmt/clippy; relay unregressed.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
