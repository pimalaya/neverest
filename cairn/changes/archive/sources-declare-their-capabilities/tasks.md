---
cairn: tasks
change: sources-declare-their-capabilities
---

# Tasks

- [x] Declare mail sources from backend and rights before any credential is read; `submit` sent only by the source it names (prototype, pimdir change `capabilities`, prototype.md).
- [x] Declare contacts and calendar sources (src/offline/capability.rs), unit tests for the calendar declaration and the one-way case.
- [x] Google Calendar: `sendUpdates` from `SCHEDULE-AGENT` on update, from the attendees on delete.
- [x] Checked against Radicale: a CalDAV source declared at sync; calendula refused an online meeting and a scheduled write once scheduling read `none`, and let a `SCHEDULE-AGENT=NONE` event through, pushed with its parameters.
- [x] Sent copies: `mail.submit.copy` declared (IMAP with SMTP files it by an `add` once sent; Gmail and Graph file it themselves, and say so on `mail.submit`); a sent `submit` asking for `copy` is replaced by that `add` and drained in the same run. Checked on Stalwart: one send, one copy in Sent Items.
- [ ] A plain SMTP server that files sent mail itself (Microsoft 365, Gmail SMTP under an IMAP source): an `smtp` setting saying so, so no second copy is added.
- [ ] A right written outside the backend table (`sources.<name>.item.delete`) is accepted and ignored, the backend being flattened into the source: refuse it (pimdir e2e.md finding 3).
- [ ] Collection rows from access rights: Google `accessRole`, Graph `canEdit`, DAV `current-user-privilege-set`, IMAP `MYRIGHTS`.
- [ ] Google Calendar: notify on a new scheduled event without losing its UID, then declare `calendar.scheduling` full.
- [ ] CalDAV: read `schedule-outbox-URL` (RFC 6638) to declare scheduling full or none instead of partial.
- [ ] Online meetings on push: Google `conferenceData.createRequest` (`conferenceDataVersion=1`), Graph `isOnlineMeeting`; strip `X-PIMDIR-ONLINE-MEETING`.
- [ ] Perform `calendar-reply` and `calendar-cancel` natively: Graph `/accept`, `/tentativelyAccept`, `/decline`, `/cancel` (io-msgraph first), Google as a `responseStatus` patch and a delete with `sendUpdates=all`; declared on each calendar the source holds, `none` source-wide (pimdir §15.6 Implementations).
- [ ] (Deferred: NLnet 2027 plan) An iMIP implementation of `calendar-reply` and `calendar-cancel` on a source with an `smtp` table, declared source-wide `partial` ("by iMIP"): the iTIP message built from the stored event (ical-rs), sent over SMTP, then the event updated (`PARTSTAT`) or removed, marked `SCHEDULE-AGENT=CLIENT`.
- [ ] (Deferred: NLnet 2027 plan) Scheduling by iMIP for a calendar source whose server does not schedule: one implementation per source, chosen in configuration (`scheduling = "server" | "imip"`), declared with its detail.
- [ ] Push modified instances on Google and Graph, then declare `calendar.occurrence.update`.
- [x] Spec: fold into cairn/spec/sync.md on landing (delta.md), log entry.
