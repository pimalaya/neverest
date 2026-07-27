---
cairn: delta
change: dotted-config-document
---

# Delta

## ADDED Requirements

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one
`[accounts.<name>]` table header per account, the only headers in the document,
with every field below it written as a dotted key. An empty table SHALL write
nothing. The saved file and the document printed on stdout SHALL be identical.

The document SHALL hold only what was actually decided: every field equal to
its default SHALL be omitted (the account `default` flag when false, the
per-side mailbox / flag / message permissions, the per-side pool size, the
mailbox filter and aliases, the HTTP-backend ALPN list, `starttls`). Omitting
a field SHALL be lossless: every skipped field keeps a deserialization default
equal to the value that was skipped.

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
