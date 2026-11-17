---
cairn: log
change: fewer-reads-a-run
date: 2026-11-17
---

# Fewer reads a run

Finding NEV-9 of the CLI family analysis of 2026-11-15, its two `[read]` rows.

## What landed

- Google Calendar reads an event's body from the calendar's last listing when that listing returned it whole (a full listing: every event with its modified instances; a resumed one: a lone event), shared by the source's connections, where it sent one or two requests per event after a listing that had returned them. A first sync of 5,000 events was 5,000 to 10,000 extra sequential requests. Any write to the calendar forgets what its listing returned.
- A side no write can reach (its rights or authority forbid every write) is listed once a run, by the opening pass, in a pair and as a lone source; the settling passes listed it again for nothing.
- Not measured: no timing was taken; the tests count requests.

## Left

- People and the Graph contacts and calendars still land a round as one page, so a run stopped mid-listing starts again from zero.
- A collection is still loaded whole from the store several times a run.
- A writable side is still listed at least twice a run (the opening pulling pass, then a pushing one).

## Spec

sync.md: the reconcile-until-quiescent paragraph, the `gcal` backend paragraph.
