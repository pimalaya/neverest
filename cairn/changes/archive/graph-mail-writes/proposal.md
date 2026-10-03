---
cairn: change
id: graph-mail-writes
status: landed
created: 2026-10-03
---

# Graph mail writes through the store

## Why

A Microsoft Graph mail source was pull-only: `mail.message.add`, `.copy` and `.move` were declared `none`, so a frontend writing through the store (himalaya on pimdir, for MOA) could neither archive nor trash by moving, nor save a draft, on a Microsoft 365 mailbox. io-msgraph already has `message_create_mime`, `message_move` and `message_copy`.

## What

- Relocate a move's remove with `message_move`. Graph changes the id; like IMAP `MOVE`, the target's next enumeration lists the member under its new id and the fetch lands the pending create by `Message-ID` (pimdir SYNC §5, §6). Immutable ids are not asked for: they would change the ids existing stores bind.
- Deliver a create carrying an origin by `message_copy`, accepted under the id Graph answers with. Other backends keep uploading.
- Append into the well-known Drafts folder only, by `message_create_mime`, then patch `\Seen` and `\Flagged`; reject an append anywhere else, Graph creating every MIME message as a draft.
- Declare `mail.message.move` and `.copy` full, `.add` partial (Drafts only), `mail.flags.draft` partial, so the producer's gate lets a draft added with `\Draft` through.
