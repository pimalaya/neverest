---
cairn: delta
change: an-imap-move-copies-without-move
---

# Delta

## ADDED Requirements

### Requirement: An IMAP move without MOVE copies, then expunges its own messages
An IMAP move SHALL send `UID MOVE` only when the server advertises MOVE (RFC 6851) or IMAP4rev2 (RFC 9051). Otherwise it SHALL `UID COPY` the UIDs into the target, then mark them `\Deleted` and `UID EXPUNGE` them alone, never by a plain `EXPUNGE`. On a server advertising no `UID EXPUNGE` either, the move SHALL fail before any command, the push staying in the store as rejected. A move interrupted after its copy SHALL be finished by the next run without a second copy, through the target search a move makes for a searchable identity; a member under a minted key, which no search finds, MAY leave a second copy in the target, never none (owner, 2026-11-21).

#### Scenario: A server without MOVE
- **GIVEN** a server advertising IMAP4rev1 and UIDPLUS but no MOVE
- **WHEN** the store moves a message from INBOX to Archive and the account is synced
- **THEN** the message is in Archive alone and no `UID MOVE` was sent

#### Scenario: A move interrupted after its copy
- **GIVEN** a move whose copy landed in the target while its source was not expunged
- **WHEN** the account is synced again
- **THEN** the source is deleted and the target holds one copy

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
