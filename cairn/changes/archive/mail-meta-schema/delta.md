---
cairn: change
change: mail-meta-schema
---

# Delta

## ADDED Requirements

### Requirement: The mail summary is a versioned schema
The `meta` a sync writes for a `message/rfc822` collection SHALL be `v: 1` JSON —
`v` (integer, required), `subject` (string, required), and optional `message_id`,
`from`, `to`, `date` (RFC 3339) and `size` (octets) — with absent optional fields
omitted (meaning "unknown"), so a reader can render an envelope list without
fetching a body. Flags SHALL NOT appear in `meta`; they are the item's flags. The
same schema SHALL be emitted by both the enumerate (`Meta`) and the streamed
(`Full`) paths, the streamed path carrying the message's known octet length as
`size`. The schema is documented in `pimdir/SPEC.md` §13.

## MODIFIED Requirements

## REMOVED Requirements
