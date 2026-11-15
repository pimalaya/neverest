---
cairn: change
id: gmail-source
status: landed
created: 2026-10-02
---

# A native Gmail source

Self-contained: a session with no prior context can take it from here. Targets the release after 0.3.0.

## Why

Gmail syncs today over IMAP, and IMAP models labels as folders. A message carrying three labels arrives three times, plus once in `[Gmail]/All Mail`. pimdir collapses the copies by Message-ID, but every run still pays for each. Gmail's IMAP has no QRESYNC, so finding flag changes means rescanning large folders.

The Gmail API holds one message with a set of labels, which is pimdir's one item filed in several collections. `users.history.list`, scoped to one label, hands each collection its own changes since a `historyId`, which fits the engine's per-source, per-collection checkpoint.

The slot exists: `gmail` parses as a source (`GmailConfig`: `user-id`, `tls`, `alpn`, `auth`) and fails when opened. io-gmail 0.4 covers what the source needs (labels, messages, history, modify and batch modify, trash, import, insert, delete), verified live against `google@pimalaya.org`.

It is not urgent. IMAP already syncs Gmail with XOAUTH2, and the API needs the same restricted `https://mail.google.com/` scope as IMAP, so going native changes nothing about OAuth verification.

## What

A `gmail` cargo feature, a `Client::Gmail` arm and `src/gmail/client.rs`, modelled on the Graph mail adapter (src/msgraph/client.rs). It syncs `message/rfc822` only. Gmail contacts and calendars stay with `gpeople` and `gcal`.

### Collections

Key and name each collection by its label name, as IMAP exposes it. Decided with the user at implementation, against the first draft (key by label id): the driver pairs endpoints and creates missing collections by key, so a label id would never meet the IMAP name of the same label, every run would create `Label_123` on IMAP and fail creating `Work` on Gmail, and a collection filter would have to name opaque ids. A renamed label reads as a remove plus an add, as on IMAP.

- User labels are collections. Nested names (`Parent/Child`) stay as Gmail writes them.
- `INBOX`, `SENT`, `DRAFT`, `SPAM` and `TRASH` are collections. `SPAM` and `TRASH` need `includeSpamTrash` on every list.
- `UNREAD` and `STARRED` are flags, not collections: no `UNREAD` means `\Seen`, `STARRED` means `\Flagged`. `IMPORTANT` maps to `$Important`, as himalaya's Gmail backend already does (himalaya src/gmail/backend.rs). Reuse those constants and that mapping rather than invent a second one.
- `CHAT` and `CATEGORY_*` are not collections. The categories partition `INBOX`, so syncing them would file every inbox message twice. Revisit only on request.
- `create_collection` and `delete_collection` map to `labels.create` and `labels.delete`, user labels only. System labels refuse.

### Enumeration

Handles are Gmail message ids. They are stable across labels, unlike Graph's, so the same handle in two collections is the same message, and no handle-space rebuild ever happens.

- **Full round** (no checkpoint, or an expired one): page `messages.list` with `labelIds=[L]`. The flags come from two more listings, `[L, UNREAD]` and `[L, STARRED]`, read as sets, plus `[L, IMPORTANT]`. That is a few id-only lists instead of one `messages.get` per message.

  Record the profile's `historyId`, taken before the first list, as the checkpoint, so nothing that lands mid-round is missed.

- **Delta round**: `history.list(startHistoryId, labelId=L)` with `messageAdded`, `messageDeleted`, `labelAdded` and `labelRemoved`, paged.

  `messageAdded`, or `labelAdded` naming L, adds the message. `messageDeleted`, or `labelRemoved` naming L, makes it vanish. A `labelAdded` or `labelRemoved` on `UNREAD`, `STARRED` or `IMPORTANT` changes flags.

  Read the current labels of every changed message in one `messages.get` per id (`format=minimal`) before reporting it, since history records are increments, not state. The new checkpoint is the response's `historyId`.

- **Membership**: a message carrying `SPAM` or `TRASH` is a member of that label only, matching the listings, which hide spam and trash elsewhere.

- **Expiry**: an expired or unknown `startHistoryId` answers 404. Restart a full round, the way the Graph adapter treats a 410 delta link. Any other error surfaces.

### Fetch

- `Meta`: `messages.get(format=metadata)` with the headers the summary needs.
- `Full`: `messages.get(format=raw)`, base64url-decoded into the body stream.

### Push

- **Flags** (`store_flags`): `messages.modify` adding or removing `UNREAD`, `STARRED` and `IMPORTANT`, or `batchModify` for several ids.
- **Delete** from collection L (`delete_item`): remove label L, which is what Gmail's own UI does. A message keeping no visible label stays in All Mail. Deleting from `TRASH` is `messages.delete`, permanent, gated by the source's `remove` right.
- **Move** L to M (`move_items`): one `modify` adding M and removing L.
- **Add** to L (`add_item_stream`): `messages.import` with `labelIds=[L]`, keeping the message's own date and `Message-ID`.

  The verified quirk applies: `import` may answer an id that 404s. The real one is found again through `history.list` since the call, matched by `Message-ID` (io-gmail's live suite has the recipe). Never report the answered id unchecked.

- `update_item_stream` refuses, a message being immutable.

## Open questions

- Should the full round's three or four id lists be one `messages.list` per label with `format=minimal` batches instead? Measure on a 50k-message account before choosing.
- Should a delete from `INBOX` archive (remove `INBOX`) or trash? Confirmed with the user: archive.

## Not in scope

Push notifications (`users.watch` and Pub/Sub, a daemon concern neverest has none of), the categories, threads as a unit, drafts beyond the `DRAFT` label, Gmail settings, and the wizard proposing Gmail over IMAP.
