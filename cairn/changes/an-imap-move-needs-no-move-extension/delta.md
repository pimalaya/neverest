---
cairn: delta
change: an-imap-move-needs-no-move-extension
---

## ADDED Requirements

### Requirement: An IMAP move respects the server's capabilities

An IMAP source SHALL use UID MOVE only when its authenticated capabilities advertise MOVE or IMAP4rev2. Otherwise, a single-message move with a searchable, non-minted identity SHALL search the destination before copying, copy only when absent, then mark the source UID Deleted and expunge that UID alone. The fallback SHALL require UIDPLUS and SHALL refuse a missing searchable identity before mutation. A failed search or copy SHALL never delete the source. A failed delete SHALL leave the move pending; a retry SHALL find the destination copy and finish the deletion without copying again. No fallback SHALL use unrestricted EXPUNGE.

#### Scenario: OVH offers UIDPLUS but no MOVE

- GIVEN an authenticated IMAP4rev1 server advertising UIDPLUS without MOVE
- WHEN a message with a searchable identity moves to another mailbox
- THEN the source searches the destination, copies the message if absent, marks the source UID Deleted and expunges only that UID

#### Scenario: Copy succeeded before deletion failed

- GIVEN a fallback copy in the destination and its source message still present
- WHEN the move is retried, including on a new connection
- THEN the destination is searched and the source is deleted without another copy
