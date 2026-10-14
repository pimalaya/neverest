---
cairn: delta
change: a-body-is-walked-once
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: The derivations are the format's
A link id, a summary and a sort key SHALL be what pimdir STORAGE Annex A gives, derived by io-pimdir: `summary::derive` at the `Full` tier from the streamed bytes, and `PimdirMailSummary` built from the IMAP or Graph envelope at the `Meta` tier, so the schema cannot drift from the format's by a field or a spelling. This crate SHALL define no summary struct and no scanner of its own.

The streamed tier reads a header prefix rather than the whole body, so it SHALL restate the message's octet length as the summary's `size`, and SHALL leave the attachment mark and the invitation unknown: neverest SHALL NOT walk a body's parts. io-pimdir's write walks each body entering the store once, from its blob, and states the mark and the invitation over the writer's summary; a body the account already holds takes those of a row holding it (Annex A.1, io-pimdir `ca4c589`). A body served from a stored blob SHALL be read up to its header block alone. The envelope tier SHALL carry every `From`, `To`, `Cc` and `Bcc` address the backend surfaces, so the address rows the two tiers write agree.

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN the summary's subject holds the decoded text, not the encoded word

#### Scenario: The mark of a downloaded body is its parts'
- **GIVEN** a `multipart/mixed` message holding only an inline image, and a `multipart/signed` one holding an attachment two levels down
- **WHEN** a sync downloads their bodies, or files them again in a second folder
- **THEN** the first is marked as attaching nothing and the second as attaching, the store's write having walked each body once (tests/attachments.rs); the streamed summary itself names them as the whole message does and states no mark (src/offline/remote.rs `scan_tests`)

## REMOVED Requirements

None.
