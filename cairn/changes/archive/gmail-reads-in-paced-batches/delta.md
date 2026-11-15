---
cairn: change
change: gmail-reads-in-paced-batches
---

# Delta

## ADDED Requirements

### Requirement: Gmail reads in paced batches
A `gmail` source SHALL read message metadata, sizes and raw bodies through Gmail's HTTP batch endpoint, `gmail.batch-size` reads a batch (10 by default, 1 to 100) and at most `gmail.batches-in-flight` batches at once across the source's connections (1 by default), each batch paced by the units of all its reads. A read refused inside a batch for concurrency, rate or a missing answer SHALL be sent again at the head of the next batch after one to two seconds, one refused a per-minute quota likewise after two to five seconds and then the next minute; a read answered 404 SHALL be a message gone, never an error; a daily quota, or a read refused six times, SHALL give the source up. A raw batch SHALL hold about 8 MiB of messages by their size estimates, a message over 5 MiB being read alone, and each body SHALL be stored as its batch lands.

A run SHALL read a message once: a message carrying several synced labels SHALL be named, under the second label listing it, from the metadata the first read, its flags from that label's own flag listings. The run SHALL read the mailbox history once, with no label, and project it on each label.

#### Scenario: Ten reads a batch, the refused one put back
- **GIVEN** a Gmail label of 45 unbound messages, one refused for concurrency once
- **WHEN** its round page is read
- **THEN** five batches are sent one after another, 10, 10, 10, 10 and 6 reads, the refused message first in the second

#### Scenario: A message under two labels
- **GIVEN** a message filed under `INBOX` and a user label
- **WHEN** both are listed in one run
- **THEN** its metadata is read once

## MODIFIED Requirements

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429, or with 503 for a request that creates nothing, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Google's refusals SHALL be read by their reasons and wording: a burst (a concurrency 429, a per-second rate, a 503) SHALL wait one to two seconds; a per-minute quota, whose answer names the minute (`Units per minute per user`) whatever its reason, two, three, four then five seconds, then the next minute; a daily quota SHALL give the source up until the next midnight in America/Los_Angeles. Past the bound, or on a stated wait longer than neverest waits, the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 3).

#### Scenario: Gmail's per-minute quota
- **GIVEN** a Gmail read answered 403 `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, reason `rateLimitExceeded`
- **WHEN** it is synced
- **THEN** the read is sent again two seconds later, rather than failing the run

### Requirement: Gmail is paced below its quota
The Gmail source SHALL hold itself to 200 quota units a second across all its connections, and to `gmail.units-per-minute` units over any sixty seconds (6,000 by default), each call, and each batch for all its reads, taking its cost before it is sent, a read costing the 5 units Gmail bills.

### Requirement: Gmail is a native mail source
(The delta and the writes, in the third and fourth paragraphs.) A delta SHALL be the mailbox history, read once a run with no label and projected on the collection's label (a record naming the label, a message the collection holds, an arrival naming no label), each touched message's current labels read before it is reported, and an expired `historyId` (404) SHALL restart a full round. A write on a message gone already (404) SHALL count as done.

## REMOVED Requirements

None.
