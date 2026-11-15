---
cairn: log
change: the-report-shapes-read-as-the-family
date: 2026-11-15
---

# The report shapes read as the family

Line X10 of the CLI alignment round of 2026-11-15, neverest's lines of the JSON shape review: A17, B9 (part), B10, B11. Line N0 landed beside it.

## What landed

- src/conflict/report.rs, src/cli/conflict.rs: a conflict's `id` is a string in `conflict list`, `show` and `resolve`.
- src/conflict/report.rs, src/conflict.rs: `conflict resolve`'s `partial` is `[{capability, source, detail}]` (`PartialCapability`), where it was sentences.
- src/sync/report.rs, src/offline/driver.rs, src/cli/exit.rs: a sync report holds `collectionPatch` and `itemPatch` arrays, entries named `CollectionHunkEntry` and `ItemHunkEntry`; `PatchOutcome` is gone; the report no longer serializes `account`.
- src/cli/init.rs: `init`'s account row carries `code`, `error` (the outermost message) and `sources` (its causes); its endpoints are `opened`, those reached before a failure included.
- Tests: `a_listed_conflict_prints_its_id_as_a_string`, `a_partial_capability_prints_as_a_row`, `a_report_lists_its_patches_as_arrays_and_names_no_account`, `a_failed_init_carries_its_causes_apart`; the local-server suites read the new keys and string ids.
- N0: .github/workflows/tests.yml runs the live job (google, microsoft) weekly (`0 5 * * 1`) and by hand, never on push; tests/google.rs works in long-lived calendars, `pimalaya-live-neverest-<purpose>` (`sync`, `meeting`, `meet`, `invitation`), created when absent, emptied at the start and end of a run, never deleted; CONTRIBUTING.md says so.

## Capabilities moved

- [sync](../spec/sync.md): *A conflict's id prints as a string* and *A report lists its patches as arrays* added; *A resolution names what the push supports in part* and *A failing row carries its code and its causes* amended.
