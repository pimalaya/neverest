---
cairn: log
change: a-series-is-pushed-whole
date: 2026-11-15
---

# A series is pushed whole

Ledger lines N22 and N23 of the CLI alignment round of 2026-11-15, from live testing (calendula part 2, P2-F5 and P2-F6).

## What landed

- src/kind/event.rs: `Occurrences`, a document's `EXDATE`s and overrides, each override as a document of its own.
- src/msgraph/client/calendar.rs: after the master, the instance of each `EXDATE` and cancelled override deleted, of each other override patched through io-msgraph's `instance_update_from_ical`; a create whose occurrences fail is deleted again.
- src/gcal/client.rs: after the master, the instance of each override updated with io-gcal's `from_ical` and `merge`; a create whose overrides fail is deleted again.
- An override of no occurrence and a `RANGE=THISANDFUTURE` override are refused coded `unsupported`.
- src/offline/capability.rs: `calendar.occurrence.update` declared `partial` on Google and Graph, where it was `none`.
- Tests on a scripted Graph server and a scripted Google session, each failing on the master-only push.

## Left

- An override or an `EXDATE` an update drops, which the server keeps, is not written back.

## Capabilities moved

- [sync](../spec/sync.md): *A series is pushed whole or not at all* added; *Microsoft Graph is a first-class source* and *Google contacts and calendars are native sources* amended.
