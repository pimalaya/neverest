---
cairn: tasks
change: an-events-only-calendar-declares-no-tasks
---

# Tasks

Track B of unfog cairn/changes/gtd-hardening (M6); uses io-pimdir cairn/changes/gtd-hardening (`capability::X_VTODO`, `PimdirReader::accepts_vtodo`), built through an uncommitted `[patch.crates-io]` path to ../io-pimdir.

- [x] `Collection::takes_tasks`, filled by the CalDAV listing from `CaldavCalendar::components`; unit test of the mapping
- [x] Sidecar `taskless` by source, recorded by a local-mode run at each listing
- [x] `declare` adds `x-vtodo` `none` on each recorded calendar the source holds; `neverest drain` reads the sidecar too
- [x] Regression test: a VTODO add into a recorded calendar is refused at the producer's gate (`x-vtodo`) and `accepts_vtodo` is false, a calendar not recorded still takes it; fails before the change
- [x] fmt, clippy `--all-targets`, `cargo test`
- [x] CHANGELOG (Unreleased)
- [ ] Fold into cairn/spec/sync.md, log, status landed: left to the user's review

## Notes

- The regression test is src/offline/driver.rs `an_events_only_calendar_refuses_a_task_before_it_is_queued`: with the `x-vtodo` rows left out of `declare`, the add into `caldav/events` is queued and the test panics at its "refused at the gate" assertion.
- The gate reads the sources syncing a collection (a checkpoint or a binding, io-pimdir `load_capabilities`), so a calendar recorded by `sync --declare-only` but never synced refuses no task until its first sync binds or checkpoints it.
