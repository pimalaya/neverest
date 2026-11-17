---
cairn: log
change: drafts-and-sent-leave-for-the-trash
date: 2026-11-17
---

# Drafts and Sent leave for the trash

Finding NEV-8 of the CLI family analysis of 2026-11-15, settled by a live check.

## Evidence

Live on the Google test account, 2026-11-17, tagged messages deleted after: `messages.import` takes `DRAFT` and `SENT` (an imported draft is listed by `drafts.list`); every `messages.modify` and `batchModify` naming either, added or removed, even on a message carrying neither, answers 400 `INVALID_ARGUMENT` "Invalid label: DRAFT" (or SENT), and a batch fails whole; adding `TRASH` alone is taken. Importing into `INBOX` a message whose `From` is the account address makes Gmail add `SENT` itself.

## What landed

- A delete from `DRAFT` or `SENT` adds `TRASH` alone (the message leaves the collection for `TRASH`), where it removed the label and was refused on every run.
- A move out of either to `TRASH` does the same; any other move into or out of either is refused coded `unsupported` before any request, so it is held rather than retried.
- The source declares `mail.message.move` `partial` and states the `DRAFT` and `SENT` rule in the details of the move and remove rows.

## Left

- A message pushed into `INBOX` from the account's own address also lands in `SENT` (Gmail's doing), which the next pull reads as a second placement.
- The Gmail copy still uploads the message again where one label add would do (the other half of NEV-8).

## Spec

sync.md, the `gmail` writes paragraph.
