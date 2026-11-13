---
cairn: delta
change: a-message-is-keyed-by-the-formats-reading
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: The derivations are the format's
At the `Meta` tier a message SHALL be named by io-pimdir's `summary::mail::derive_meta` over the header fields its source serves raw, by the same read on a listing and on a `Meta` fetch: IMAP's `BODY.PEEK[HEADER.FIELDS (…)]`, never an `ENVELOPE`; Gmail's metadata headers laid out as a header block, each field from its first entry. A Graph row is named from the values Graph decodes, which stand until the body is read, its `internetMessageId` read by io-pimdir's `message_id`; `internetMessageHeaders` is not read, io-msgraph 0.7 not modelling it. This crate SHALL define no summary struct and SHALL parse no `Message-ID`, `In-Reply-To`, `Subject`, `Date` or address field itself. A hint already derived SHALL be compared as it stands.

#### Scenario: A Message-ID wrapped in comments
- **GIVEN** the message of pimdir vectors/fixtures/mail-id-comment.eml
- **WHEN** it is listed over IMAP, over Gmail or over Graph
- **THEN** it is keyed `comment-1@example.org`, as vectors/summaries.json states

#### Scenario: A folded subject and no Message-ID
- **GIVEN** the message of vectors/fixtures/mail-folded-subject.eml
- **WHEN** it is listed over IMAP or over Gmail
- **THEN** its `alt:` key holds the subject unfolded, joined and trimmed, as the vectors state

### Requirement: Bodies are content-addressed and deduped
The `Meta` tier of `message/rfc822` reads the header fields its source serves, where the text named the IMAP ENVELOPE.

### Requirement: The summary is the format's typed row
The `Meta` path is the listed one, where the text named the envelope.

## REMOVED Requirements
