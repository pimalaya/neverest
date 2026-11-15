---
cairn: change
change: sync-safety-fixes
---

# Delta

## ADDED Requirements

### Requirement: A Graph mail is dated by its `Date`
The Graph mail source SHALL fill a message's summary `date` from `sentDateTime`, Graph's reading of the `Date` header, which pimdir's mail `date` column and sort key hold (STORAGE Annex A.1). A message without one SHALL have no date, never its `receivedDateTime`.

#### Scenario: A mail delivered late
- **GIVEN** a Graph message sent at 12:00 and received at 12:05
- **WHEN** its delta row is folded into a summary
- **THEN** its date is 12:00

### Requirement: An IMAP delete expunges its own message alone
An IMAP delete SHALL mark the UID `\Deleted` and expunge it by `UID EXPUNGE` on that UID (RFC 4315 UIDPLUS, built into IMAP4rev2), never by a plain `EXPUNGE`, which removes every message any client marked `\Deleted` in the mailbox. On a server advertising neither, the delete SHALL fail before any flag is stored, the push staying in the store as rejected.

#### Scenario: Another client's deleted message survives
- **GIVEN** a mailbox holding two messages, one marked `\Deleted` by another client
- **WHEN** the store deletes the other one and the account is synced
- **THEN** the deleted one is gone from the server and the one the other client marked is still there

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429 or 503, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Past the bound, or on a stated wait longer than neverest waits, the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 3).

#### Scenario: A server answering 503
- **GIVEN** an account whose CalDAV source answers every request 503
- **WHEN** it is synced
- **THEN** the run exits 3 and `throttled` names `caldav` with an `until`

### Requirement: Gmail is paced below its quota
The Gmail source SHALL hold itself to 200 quota units a second across all its connections, about 40 `messages.get` or `messages.list` a second against Gmail's 250 a user, each call taking its listed cost before it is sent.

## MODIFIED Requirements

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code: a source it could not scan, a hunk it could not apply, a send or an intent that failed without parking, or a source that gave up throttled. It SHALL win over the conflict code, the report still counting what waits for a person.

## REMOVED Requirements

None.
