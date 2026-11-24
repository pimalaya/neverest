---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A Graph event is created without an empty body

Task N3 of the-live-night-findings. On the live night of 2026-11-23, a recurring event pushed to Graph by one drain, then deleted through calendula, came back on the next drain: the delete met a revision neverest had not pushed, and the edit won (pimdir SYNC §5).

The proposal blamed the series. A raw probe on a throwaway calendar on 2026-11-24 showed otherwise: Exchange rewrites any event 0.5 to 4 seconds after a write setting its body (text or HTML, empty or not), moving only its `changeKey` and `lastModifiedDateTime`. A create with no body, and a patch of the subject alone, move nothing. io-msgraph's `create_from_ical` always sends a body, an empty text one for an event without `DESCRIPTION`, so every event neverest created was rewritten after the read back. A single event reproduces it too.

neverest now leaves an empty body out of a Graph create (src/msgraph/client/calendar.rs `is_empty_body`). Capability moved: sync.md, "A Graph source syncs mail, contacts and calendars" (calendars paragraph).

Left open: an event created or updated with a description is still rewritten, and a delete queued before the next run still loses. A create reporting no revision does not help (tried: the next listing still shows a revision the base lacks), and a base read back after the create cannot, the rewrite landing after the read. It needs a ruling (a bounded wait for the rewrite after a body write, or N2's "delete overruled" report).

## Checked

- tests/msgraph.rs `a_graph_series_deleted_right_after_its_create_is_deleted`: fails with the fix disabled ("the series came back"), passes with it.
- src/msgraph/client/calendar.rs `an_event_without_description_is_created_with_no_body`: fails with the fix disabled, passes with it.
