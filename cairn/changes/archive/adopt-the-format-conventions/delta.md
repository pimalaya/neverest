---
cairn: delta
change: adopt-the-format-conventions
---

## ADDED Requirements

### Requirement: The conventions are the format's, the readers are not
A link id, a summary and a sort key SHALL be what pimdir SPEC Annex A and the
format's `vectors/meta.json` give, and the summary SHALL be
`io_pimdir::conventions`'s own type (`PimdirMailMeta`, `PimdirCardMeta`), so the
schema cannot drift from the format's by a field or a spelling. This crate SHALL
NOT define a summary struct of its own.

The **scanners** that read those fields off a body stay here while io-pimdir's
lose data these do not, and each gap SHALL be held by a test naming it:

- `conventions::mail` reads headers raw, so an RFC 2047 encoded-word subject
  reaches a reader as `=?utf-8?q?…?=`;
- `conventions::card` splits a property on the first colon, cutting the value of
  a legal quoted parameter that holds one (RFC 6350 §3.3), and leaves RFC 6350
  §3.4 escaping in place.

The format's vectors are ASCII-only and cover neither, so nothing upstream
reports the difference. When io-pimdir closes a gap, its `derive` SHALL replace
the scanner rather than be mirrored beside it.

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN `meta.subject` holds the decoded text, not the encoded-word

## MODIFIED Requirements

### Requirement: Bodies are content-addressed and deduped
An item body SHALL be stored once per content hash; an item present on both
sources or in several collections is stored once and copied by reference. The
link id SHALL be the identity pimdir SPEC Annex A gives, with nothing prepended:
the bare `Message-ID` for mail, the bare `UID` for a card. Only a kind's own
fallback is marked (`alt:` over subject, date and sender; `hash:` over a card's
body), that being the one case a prefix is for, a name no server has heard of; a
real id cannot be mistaken for one, RFC 5322 `atext` admitting no colon before
the `@`.

Where a kind resolves its link id at more than one tier — `message/rfc822`, from
the IMAP ENVELOPE at `Meta` and from the parsed body at `Full` — the two
derivations MUST produce the byte-identical string for the same item. In
particular the date component SHALL be formatted the one canonical way, the
**UTC instant** in RFC 3339 at seconds precision, so a message with no
`Message-ID` does not link one way at `Meta` and another at `Full`. Kinds
resolving at a single tier (the DAV kinds) cannot hit this class of bug.

### Requirement: The mail summary is a versioned schema
The `meta` written for a `message/rfc822` item SHALL be `v: 1` JSON — `v`
(required), `subject` (required), and optional `message_id`, `in_reply_to`,
`from`, `to`, `date` and `size` (octets), with absent optionals omitted — so a
reader can render an envelope list without fetching a body. `date` SHALL be the
UTC instant in RFC 3339, never the local reading the sender wrote, which is what
lets two writers of one store compare and order items without re-parsing.
Flags are not in `meta`. Both the enumerate (`Meta`) and the streamed (`Full`)
paths SHALL emit this schema, the streamed path carrying the message's known
octet length as `size` rather than the header prefix it read. The schema is
`PimdirMailMeta`, documented in `pimdir/SPEC.md` Annex A.

## REMOVED Requirements

None.
