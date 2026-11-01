---
cairn: change
change: a-listing-is-read-as-the-server-stated
---

# Delta

## ADDED Requirements

### Requirement: An IMAP mailbox is never read as smaller than it stated
A mailbox whose `SELECT` answer states no `EXISTS` SHALL be searched as any other, never read as empty: a listing read short is a delete of everything left out.

### Requirement: A Graph band page resumes below a date
A Graph band page SHALL resume below the second the page before it ended at, that second included, never at the offset Graph's next link carries: a member deleted between two pages, in one run or across two, would shift a later one out of the round, and a band round has no pass to list it again. A page whose messages were all sent within one second SHALL follow Graph's next link within the same request until a page leaves that second. A band next link a round stored before SHALL be refused, which opens the round again.

#### Scenario: A message deleted between two runs of a band round
- **GIVEN** a band round whose first page landed, then a message of that page deleted on Graph
- **WHEN** the next run resumes the round
- **THEN** every message the band still holds is listed, none shifted out of it

## MODIFIED Requirements

### Requirement: A mail round lands page by page, newest first
In its first paragraph, the Graph cursor under a scope reads "its cursor the second the page ended at, the next page listing below the second after it" in place of "its cursor the next link", and the list of refused cursors reads "(an expired Graph link, a Gmail page token)", a moved `UIDVALIDITY` being a rekey (see the IMAP `UIDVALIDITY` requirement).

### Requirement: A body gone from the server is owed nothing more
A body read the server answers gone SHALL NOT fail the run: a Gmail or Graph read answered 404 (in a batch, a size read, or alone), a CardDAV, CalDAV, Google Calendar or Google People read answered 404 or 410, and an IMAP UID a completed `UID FETCH` answers nothing for (RFC 9051 §6.4.9), or a lone fetch of it that streams nothing. The run SHALL record the body gone for each collection owing it and SHALL NOT ask for it again: no later batch of phase 1 and none of phase 2. The store SHALL keep the member as it was listed, a body read being no listing (pimdir SYNC §5), so the next run's listing takes it away before that run plans its body; a member the listing names and every read finds missing is thus asked for once a run, never failing its collection.

## REMOVED Requirements

None.
