---
cairn: change
id: an-imap-move-needs-no-move-extension
status: landed
created: 2026-11-21
---

# An IMAP move needs no MOVE extension

## Why

OVH advertises IMAP4rev1 and UIDPLUS but no MOVE. Neverest sends UID MOVE regardless, receives BAD, and repeated refusals can end the connection before the rest of the sync runs. The user reviewed the diagnostic and requested this upstream fix after the deployment handoff.

## What

Select native UID MOVE only on MOVE or IMAP4rev2 servers. Otherwise, require UIDPLUS and a searchable, non-minted message identity before copying, then use UID COPY, UID STORE and targeted UID EXPUNGE. Search the destination on each attempt so a copy made before an interrupted delete is not made again. Preserve native moves for identities that cannot be searched, and refuse their non-native fallback before mutation.

Use the existing io-imap operations, leaving protocol encoding in the library. Add scripted loopback tests for native selection, fallback order, failures and retry, and document that live OVH validation remains external.
