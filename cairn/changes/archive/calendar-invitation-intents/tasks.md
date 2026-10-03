---
cairn: tasks
change: calendar-invitation-intents
---

# Tasks

- [x] src/offline/invitation.rs: the two intents read from the queue, their `v: 1` payload (item by `seq` or `link_id`), failures classified (a refusal or a 4xx parks, 408, 412, 429, a 5xx or a transport error retries).
- [x] src/offline/driver.rs: `drain_invitations` after `drain_submits` on both paths; the item located through its binding on the performing source, a pending create waiting for its push, a gone item parking; `IntentEntry` in the report.
- [x] src/msgraph/client/calendar.rs: `reply_event`, `cancel_event` through the event actions (raw `MsgraphSend`, io-msgraph has no coroutine for them).
- [x] src/gcal/client.rs: `reply`, `cancel`; a new scheduled event the account organises inserted with its `iCalUID`.
- [x] src/offline/capability.rs: the intents declared per held calendar, Google cancel `partial`, Google scheduling detail updated, CalDAV details pointing at RFC 6638.
- [x] Unit tests (payloads, classification, Google reply patch, declarations, drain survival, gone and pending items); live tests on Google and Graph (tests/google.rs, tests/msgraph.rs).
- [ ] CalDAV: read `calendar-user-address-set` (RFC 6638) to perform both as the write the server schedules from, declared `partial` (the comment is lost).
- [ ] Graph: a mailed invitation reads back under Exchange's global object id (`040000008200E000…vCal-Uid…`), the original UID hex-encoded inside it; io-msgraph's projection could unwrap it.
- [ ] Modified instances on Google and Graph (`calendar.occurrence.update`).
