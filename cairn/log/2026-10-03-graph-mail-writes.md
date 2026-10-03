---
cairn: log
change: graph-mail-writes
date: 2026-10-03
---

# Graph mail writes through the store

A Graph mail source now pushes moves, copies and drafts, so a frontend writing through the store archives, trashes and saves drafts on a Microsoft 365 mailbox.

## What landed

- src/msgraph/client.rs: `move_messages` (`message_move`), `copy_message` (`message_copy`), and appends into the well-known Drafts folder only (`message_create_mime`, then `\Seen` and `\Flagged` patched in).
- src/client.rs, src/offline/remote.rs: `Client::copy_item`; an `Add` carrying an origin is delivered by server-side copy where the backend copies (Graph mail), by upload elsewhere as before.
- src/offline/capability.rs: `mail.message.move` and `.copy` full, `.add` partial (Drafts only), `mail.flags.draft` partial.
- tests/msgraph.rs (ignored, live): a move bounced between the Inbox and a throwaway folder then to Deleted Items, once on the server and in the store under one `seq` after every run, with both halves seen delivering; a draft added once, and an add outside Drafts rejected.

## Capabilities moved

- sync: "Graph mail moves, copies and adds drafts" added; "Microsoft Graph is a first-class source" amended.
