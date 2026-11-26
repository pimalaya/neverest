---
cairn: tasks
change: a-move-into-a-same-id-message-keeps-both
---

# Tasks

- [x] A Stalwart test, failing first (repro.patch, 2026-11-21): both variants fail on the bound bodies, none on the server counts.
- [x] io-pimdir proposal: io-pimdir cairn/changes/a-same-id-message-keeps-its-body; the owner chose option (b) for linking and approved the relocation handle (2026-11-21).
- [x] neverest: hand the `COPYUID` of a move to io-pimdir (`ImapBackend::move_messages`, `Client::move_items`, `relocated` in src/sync/remote.rs), for `UID MOVE` and for `UID COPY` then `UID EXPUNGE`.
- [x] repro.patch landed in tests/moves.rs (2026-11-21). Its unlisted variant passes against io-pimdir with the relocation handle, and fails with the `COPYUID` dropped.
- [ ] Its listed variant still fails (re-checked 2026-11-26: ef809a9 still locks io-imap 0.12.0 on imap-types 2.0.0-alpha.8, no `EmailId`): neverest states no server object id, io-imap 0.12.0 (imap-types 2.0.0-alpha.8, imap-codec 2.0.0-alpha.10) having no `EMAILID` fetch item to request nor to parse. Stalwart announces `OBJECTID`. Needs `MessageDataItemName::EmailId`, `MessageDataItem::EmailId` in imap-types, their parse and encode in imap-codec, then the listing's `FETCH` asking it and `PimdirRemoteMeta::with_server_object_id`.
- [x] Fold, log, CHANGELOG (2026-11-21).
