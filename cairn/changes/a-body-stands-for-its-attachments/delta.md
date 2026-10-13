---
cairn: delta
change: a-body-stands-for-its-attachments
---

# Delta

## ADDED Requirements

### Requirement: A landed body stands for its attachments
Every body a run lands SHALL reach the store through io-pimdir's write (`PimdirSourceStore::write`, the `WantsWrite` of a sync, an upgrade or a mutation), which records in the same transaction a file holding no body per part of a message disposed as an attachment, in the account's attachments collection, with its name, media type, decoded size and section, and an `attachment` reference from the message (pimdir STORAGE §14.3, io-pimdir `14a8fca`). neverest SHALL write the store by no other way. A body the account already holds (a copy, a move, a second Gmail label) records none, and a body held before is not walked again.

#### Scenario: Primed, paged or after the listing
- **GIVEN** an IMAP folder holding a message with two attachments, one with an inline image only, one with an attachment two levels down a `multipart/signed`, and a plain one
- **WHEN** a sync downloads their bodies, primed, by page over four connections, or after the listing over one
- **THEN** the account holds three stand-ins, each named, typed and sized, referenced by its message; a second sync fetches and records nothing, and a copy of the messages in another folder adds no stand-in (tests/attachments.rs `a_synced_body_stands_for_its_attachments`)

#### Scenario: A body landing later
- **GIVEN** a run stopped once its listing landed, every body owed
- **WHEN** a message with an attachment arrives and a `--bodies listed` run follows, then a full run
- **THEN** the arrival's stand-in is recorded by the first, the backlog's by the second (tests/attachments.rs `a_body_landing_later_stands_for_its_attachments_then`)

## MODIFIED Requirements

### Requirement: The derivations are the format's
A link id, a summary and a sort key SHALL be what pimdir STORAGE Annex A gives, derived by io-pimdir: `summary::derive` at the `Full` tier from the streamed bytes, and `PimdirMailSummary` built from the IMAP or Graph envelope at the `Meta` tier, so the schema cannot drift from the format's by a field or a spelling. This crate SHALL define no summary struct and no scanner of its own.

The streamed tier reads a header prefix rather than the whole body, so it SHALL restate the message's octet length as the summary's `size`. It SHALL feed the whole body, as it streams, to io-pimdir's walk of the parts (`PimdirAttachmentWalk`, bounded: a header block, a line head and a boundary per level, never the body), and SHALL carry the attachment mark it gives, the one `summary::derive` reads off the whole message; a body served from a stored blob SHALL be walked from the blob alike. The invitation stays unknown on the streamed tier. The envelope tier SHALL carry every `From`, `To`, `Cc` and `Bcc` address the backend surfaces, so the address rows the two tiers write agree.

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN the summary's subject holds the decoded text, not the encoded word

#### Scenario: The mark of a streamed body is its parts'
- **GIVEN** a `multipart/mixed` message holding only an inline image, and a `multipart/signed` one holding an attachment two levels down
- **WHEN** their bodies stream into the store, in chunks of any size, or are served from a stored blob
- **THEN** the first is marked as attaching nothing and the second as attaching (src/offline/remote.rs `mark_tests`, tests/attachments.rs)

## REMOVED Requirements

None.
