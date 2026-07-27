---
cairn: change
change: stable-alt-link-id
---

## MODIFIED Requirements

### Requirement: Bodies are content-addressed and deduped
A message body SHALL be stored once per content hash; a message present on both
sides or in several mailboxes is stored once and copied by reference. The link id
is the `Message-ID`, falling back to a `(subject, date, sender)` digest. The link
id SHALL be computed identically regardless of which tier resolves it — the `Meta`
tier from the IMAP ENVELOPE and the `Full` tier from the parsed body MUST produce
the byte-identical string for the same message. In particular the date component
SHALL be formatted the one canonical way (`to_rfc3339` with UTC written as `Z`,
an offset as `+hh:mm`, seconds precision), so a message with no `Message-ID` does
not link one way at `Meta` and another at `Full` — which would strand the `Meta`
item (re-fetched every sync) and store its body under a second link id.
