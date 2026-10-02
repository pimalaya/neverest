---
cairn: tasks
change: gmail-source
---

# Tasks

## 0. Decide

- [x] Confirm with the user: a delete from `INBOX` archives, not trashes.
- [ ] Measure the full round (id lists against minimal batches) on a large account, and pick one. Shipped with the id lists, unmeasured.

## 1. Wiring

- [x] Cargo: `gmail = ["dep:io-gmail", "io-gmail/client"]`, in `default`, with the TLS and `vendored` forwards; io-gmail 0.4.
- [x] `Client::Gmail(Box<GmailClient>)`, every `match` arm, and `open` building it from `GmailConfig` (bearer token through the secret resolver, as `gpeople` does). `media_type` is `message/rfc822`.
- [x] Remove "JMAP and Gmail configs parse but do not open yet" from the src/client.rs header for Gmail.

## 2. Adapter (src/gmail/client.rs)

- [x] `list_collections`: user labels plus `INBOX`, `SENT`, `DRAFT`, `SPAM`, `TRASH`, keyed and named by label name, with counts on request.
- [x] `create_collection` and `delete_collection` on user labels, a system label refusing.
- [x] `enumerate`: the full round with its pre-taken `historyId` checkpoint, the delta round over `history.list(labelId)`, and the 404 restart.
- [x] Flags: `UNREAD`, `STARRED`, `IMPORTANT` both ways, sharing himalaya's constants and mapping.
- [x] `fetch_summaries` (`format=metadata`) and `fetch_bodies` (`format=raw`, base64url).
- [x] `store_flags`, `delete_item`, `move_items` as label changes, with `messages.delete` from `TRASH` only.
- [x] `add_item_stream` through `messages.import`, re-finding the real id through history; `update_item_stream` refusing.

## 3. Tests

- [x] Unit tests on the pure parts: label to collection or flag, history record to enumeration entry, import-id recovery.
- [x] A live run against `google@pimalaya.org` (service account with domain-wide delegation, scope `https://mail.google.com/`). Done as tests/google.rs: two replicas of one Gmail account over a throwaway label, an import crossing, a star crossing back, a full round, and a removal archiving. The Gmail-and-IMAP two-source run was not done.

## 4. Land

- [x] Fold the delta into cairn/spec/sync.md, log entry, status `landed`. Not archived while the measurement is open.
- [x] README backend list, config.sample.toml (`gmail` block documented, left commented beside the active IMAP example), CHANGELOG `Added`.
- [x] fmt, clippy `-D warnings` with `gmail` alone and with all features.
