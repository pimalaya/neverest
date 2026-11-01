---
cairn: change
id: a-flag-push-names-what-it-changes
status: landed
created: 2026-10-30
---

# A flag push names what it changes

## Why

pimdir sync-draft-05 (SYNC §4) and io-pimdir's a-queued-edit-names-what-it-changes turned the engine's flag push into deltas: `PimdirChangeKind::SetFlags { handle, add, remove }` replaces `{ handle, flags }`. neverest pushed a whole set with a replace (`FlagOp::Set`: an IMAP `STORE FLAGS`, a Graph patch of `isRead` and the follow-up status together, a Gmail modify naming `UNREAD`, `STARRED` and `IMPORTANT`), so a flag another client changed between the listing and the push was overwritten with the value the run had listed.

## What

- `Client::update_flags(collection, ids, add, remove)` replaces `store_flags`; the push applies the engine's two sets as they come.
- IMAP: a `+FLAGS` store for `add` and a `-FLAGS` store for `remove`, each skipped when empty.
- Graph: a patch naming only the properties a flag of either set maps to (`isRead`, `flag`), none sent when neither does.
- Gmail: one modify naming only the labels the two sets map to.
- DAV, Google Calendar and Google People keep refusing a flag push (they carry no flags, and the flag right is withheld for their kinds).
- The fake IMAP server applies `UID STORE`, so the connector test reads the flags it holds after the push.
