---
cairn: delta
change: graph-mail-writes
---

# Delta

## ADDED Requirements

### Requirement: Graph mail moves, copies and adds drafts

An `msgraph` source relocates a move's remove with `message_move` and delivers a create carrying an origin by `message_copy`, accepted under the id the copy answers with. A relocation reports no handle: the target's next enumeration lists the member under its new id, whose fetch lands the pending create by its `Message-ID`. An append lands in the well-known Drafts folder only, its flags patched in after, and is rejected anywhere else. The source declares move and copy full, add and `mail.flags.draft` partial.

## MODIFIED Requirements

### Requirement: Microsoft Graph is a first-class source

Graph mail pushes moves, copies and appends into Drafts beside flag changes and deletes; content updates stay rejected.

## REMOVED Requirements
