---
cairn: change
id: a-message-is-keyed-by-the-formats-reading
status: landed
created: 2026-11-13
---

# A message is keyed by the format's reading

## Why

pimdir audit line D2 (pimdir cairn/changes/audit-2026-11-12/tasks.md): neverest read a message's identity fields with parsers of its own where pimdir STORAGE Annex A.1 binds every writer. `normalize_message_id` stripped one pair of angle brackets, so the `Message-ID` of pimdir vectors/fixtures/mail-id-comment.eml was keyed by its whole commented text; a targeted IMAP summary read took an `ENVELOPE` through chrono and rfc2047_decoder. The Android app keys the same messages through io-pimdir's `derive_meta`, and a key is never derived again, so the two disagreed for ever.

## What

- IMAP: the `Meta` fetch reads the header fields a listing reads (`BODY.PEEK[HEADER.FIELDS (…)]`, `RFC822.SIZE`) through `derive_meta`, never an `ENVELOPE`. The `ENVELOPE` readers go, with rfc2047-decoder.
- Gmail: the metadata headers are laid out as a header block, each field from its first entry, and read by `derive_meta`, as the Android app reads them.
- Graph: `internetMessageId` is read by io-pimdir's `message_id`. The subject, date and addresses Graph decodes stand as it states them (Annex A.1).
- `ItemSummary`, `Address`, `normalize_message_id`, `parse_message_ids`, `kind::mail::parse_summary` and `Kind::parse_summary` go: every connector hands io-pimdir's own derivation.
- Ten pimdir fixtures are copied under tests/vectors/fixtures and each connector's path is held to what vectors/summaries.json states for them.

## Not in this change

- Graph's `internetMessageHeaders`, the raw read Annex A.1 names: io-msgraph 0.7 does not model the property.
- JMAP: neverest opens no JMAP source yet.
