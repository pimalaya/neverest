---
cairn: delta
change: a-graph-message-is-read-raw
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: The derivations are the format's
What it says of Graph is replaced. A Graph source SHALL select `internetMessageHeaders` on every read naming a member: the whole-folder delta, a band page, a listing by arrival, the summary batch of a delta or of the pass, and a `Meta` fetch. A row listing header fields SHALL be named off them by `derive_meta`, the fields a summary reads (`Message-ID`, `In-Reply-To`, `Subject`, `From`, `To`, `Cc`, `Bcc`, `Date`, `Content-Type`) laid out as the header block they came from, each from its first entry of that name; its size and its attachment mark stay Graph's own, the mark being the top-level `Content-Type`'s where Graph states none. The pass closing a round under a scope names no member and SHALL select ids, dates and flags alone.

A row listing no header field, its read having selected none or Graph holding none for the message, SHALL be named from `subject`, `sentDateTime` and the addresses as Graph states them, its `internetMessageId` read by io-pimdir's `message_id`, as before this change. The scope of a Graph listing is still read off `sentDateTime`; the `date` a row is named by is its `Date` field's.

The sentence saying `internetMessageHeaders` is not read, io-msgraph 0.7 not modelling it, is removed.

#### Scenario: A folded subject and no Message-ID
- **GIVEN** the message of vectors/fixtures/mail-folded-subject.eml
- **WHEN** it is listed over IMAP, over Gmail, or over Graph by a row listing its header fields
- **THEN** its `alt:` key holds the subject unfolded, joined and trimmed, as the vectors state

#### Scenario: A Graph row listing its header fields
- **GIVEN** a Graph row whose `internetMessageHeaders` lists the fields of one of the ten fixtures, repeated names kept in their order, beside a `subject`, a `sentDateTime`, a `from` and an `internetMessageId` stating another message
- **WHEN** the row is named
- **THEN** its key, its `subject`, `date`, `sender`, `sender_name` and `in_reply_to` are those vectors/summaries.json states for the fixture

#### Scenario: A Graph row listing no header field
- **GIVEN** a Graph row carrying no `internetMessageHeaders`
- **WHEN** the row is named
- **THEN** it is named from `internetMessageId`, `subject`, `sentDateTime` and `from` as Graph states them

## REMOVED Requirements
