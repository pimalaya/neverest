---
cairn: delta
change: live-submission
---

## ADDED Requirements

### Requirement: A submission greets with an address literal
An SMTP submission session SHALL greet with the loopback address literal
(`EHLO [127.0.0.1]`), the form RFC 5321 §4.1.3 reserves for a client with no
resolvable domain name of its own, which a desktop client behind a NAT never
has. It SHALL NOT greet with a bare `localhost`, which is not such a name
either: RFC 5321 §4.1.4 entitles a server to check, and one that does (Stalwart)
answers `550 5.5.0 Invalid EHLO domain`, failing the session before `MAIL FROM`
and leaving every intent pending behind a warning.

#### Scenario: A checking server takes the greeting
- GIVEN a queued `submit` intent and a side whose `smtp` channel points at a server that validates the EHLO argument
- WHEN the sync performs the intent
- THEN the session opens, the message is accepted, and the queue row is acknowledged

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
