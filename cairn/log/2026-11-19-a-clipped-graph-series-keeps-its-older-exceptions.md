---
cairn: log
change: connector-listings-fail-rather-than-shrink
date: 2026-11-19
---

# A clipped Graph series keeps its older exceptions

The owner's ruling of 2026-11-19 on the clipped series of 2026-11-19-an-open-graph-series-older-than-its-window-says-so: an exception before the floor is unknown on the Graph side, not removed. Before, a base holding a 2012 override (pushed from CalDAV) met a Graph body read from the floor without it, and the run took it as deleted on Graph and deleted it on the other endpoint.

## What landed

- src/calendar.rs: `carry_before` completes a read body with the overrides and `EXDATE` values its base sets apart before the floor and it lacks, with the `VTIMEZONE`s they name; `OccurrenceId::before` reads a date, an instant, or a wall time in no zone known (counted before until 14 hours past the floor).
- src/sync/remote.rs: `PimRemote::fetch_full`, which every Graph calendar body read of the engine goes through (the listing naming by body, a conflict's upgrade), completes each body the run noted clipped from its placement's base object (`BoundMeta::object`, from `Bound::from_loaded`), writing the completed body as a new blob before the engine compares it. A series with no base, or none noted clipped, is left as read.
- src/msgraph/client.rs `SeriesClips::floor`, src/client.rs `Client::clip`: the floor a series was noted read from.
- src/sync/driver/reconcile.rs: a conflict's upgrade is given the bindings, so its fetch finds the base too.
- Pushes are unchanged: the master is patched against Graph's own read and an occurrence the document leaves out is never written back, so no push rewrites a pre-floor exception away; a carried one is found by a listing around its date and patched only where it differs (one listing each per push).
- Tests: `an_exception_before_the_floor_of_a_clipped_series_is_not_deleted` (src/sync/driver/graph_clips.rs: a Graph endpoint and another endpoint in one store, the 2012 override kept and sent, next year's override deleted, the series still noted clipped), failing on the code before (the update sent lacked the 2012 override); `a_read_lacking_what_its_base_sets_apart_before_the_floor_gets_it` (src/calendar.rs: override, `EXDATE`, zone carried, post-floor ones not, idempotent).

Left: a rule or start changed on Graph is not compared against the base, so carried exceptions stay until the other endpoint removes them; the hydrate passes (src/sync/driver/hydrate.rs) carry nothing, a Graph calendar body being read with its listing.

## Spec

- [sync](../spec/sync.md): *An open Graph series older than its window says so* extended with the carry-over, its push side and the scenario *A 2012 override the window leaves out*.
