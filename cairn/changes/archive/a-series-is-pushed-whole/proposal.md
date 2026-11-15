---
cairn: change
id: a-series-is-pushed-whole
status: landed
created: 2026-11-15
---

# A series is pushed whole

## Why

Live testing of 2026-11-15 (pimalaya/.github cairn/changes/live-testing-2026-11-15, calendula part 2, P2-F5 and P2-F6; ledger lines N22 and N23 of cli-alignment-2026-11-15, high): a daily series queued by calendula with an `EXDATE` and one override (store seq 6403 on Graph, 6401 on Google) was pushed as its master alone.

- Graph: the master landed, the excluded occurrence and the override did not; the next sync read the series back from Graph and rewrote the store copy, so the exception and the override were lost on both sides, with no report line.
- Google: one `events.import`, the `EXDATE` kept in the recurrence, the override dropped; the store kept the override, so store and server disagreed silently.

Both sources wrote the series master only ("an exception edited locally does not push", "an instance modified locally does not push yet").

## What

Pushed whole, the libraries allowing it today:

- io-msgraph 0.9 offers `MsgraphEvent::instance_update_from_ical`, which projects one occurrence onto the update of its Graph instance, and documents that a consumer deletes the instance of an occurrence the document cancels. The Graph source creates or patches the master, then deletes the instance of each `EXDATE` and each `STATUS:CANCELLED` override and patches the instance of each other override.
- io-gcal 0.5's `GcalEvent::from_ical` reads a document holding one occurrence alone as that occurrence (its `RECURRENCE-ID` as `originalStartTime`) and `merge` keeps the instance the server copy names. The Google source imports, inserts or updates the master, the `EXDATE`s riding its recurrence, then updates the instance of each override.
- src/kind/event.rs reads a document's occurrences (`Occurrences`): the master's `EXDATE` values and each override as a document of its own.
- What neither provider takes is refused, coded `unsupported`, naming the occurrence: an override of an occurrence the series does not have, and a `RANGE=THISANDFUTURE` override (refused before anything is written). A create whose occurrences do not all land is deleted again and refused, so the store keeps its body and the next run tries again.
- Both sources declare `calendar.occurrence.update` `partial`, where they declared `none` ("does not push yet"): an update changing an occurrence now pushes, and one dropping an override (or, on Graph, an `EXDATE`) leaves it on the server.

## Decisions

- **Push whole, not refuse.** Both libraries carry an occurrence through its instance today; no library change is needed.
- **A create is taken back on failure.** Reporting a create as done while an occurrence failed is the silent loss this fixes; the create is deleted again, as a create whose UID Graph dropped already is.
- **What the document no longer sets apart is left alone.** An update dropping an override or an `EXDATE` the server holds is not written back: Graph has no way to restore a cancelled occurrence, and the instances a series reads are windowed, so a refusal there would also refuse legitimate updates of open-ended series. Left open.
