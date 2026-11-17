---
cairn: change
id: a-relay-fails-one-message-at-a-time
status: landed
created: 2026-11-17
---

# A relay fails one message at a time

## Why

Finding NEV-2 (HIGH) of the CLI family analysis of 2026-11-15, re-checked on 96093d8:

- (a) A target refusing the append of a message larger than the pipe left the run hung for good, the fetch thread blocked on a full pipe nobody read. 8c28411 (`std::io::pipe`, whose reader is dropped with the append) already closed it; no test held it.
- (b) A message the target refuses failed the whole collection at the same message on every run (src/sync/driver/relay.rs, `relay_copies`), the messages after it never relayed; before every-failure-reaches-the-result the run even exited 0.
- (c) The append is length-prefixed with the listing's size and nothing compared it with the octets streamed: a longer body was appended cut and bound as a good copy, a shorter one left the connection inside a literal.
- Neither side's connection was recovered after a failed relay, so a fetch cut by a closed pipe left its answer unread for the next command.

## What

- The fetch streams through a sink passing exactly the listed size, its last octet held until the source is known to end there, so a body of another length never completes an append (`SizeMismatch`).
- After a failed relay each side that failed is recovered as a failed body fetch is (`Client::recover`: a server refusal keeps the connection, anything else reopens it).
- A message that failed alone is reported under `rejected` (action `append`, its id the link) and the relay goes on with the next one (exit 3, retried by the next run); only a side that cannot be recovered fails the collection.
