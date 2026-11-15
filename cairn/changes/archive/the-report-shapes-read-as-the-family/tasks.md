---
cairn: tasks
change: the-report-shapes-read-as-the-family
---

# Tasks

- [x] A17: conflict ids as strings (src/conflict/report.rs, src/cli/conflict.rs); the local-server suites read them as strings (tests/calendars.rs, tests/carddav.rs, tests/endpoints.rs, tests/merger.rs)
- [x] B10: `partial` as `PartialCapability` rows (src/conflict/report.rs, src/conflict.rs)
- [x] B11: `collectionPatch` and `itemPatch`, `CollectionHunkEntry` and `ItemHunkEntry`, no `report.account` (src/sync/report.rs, src/offline/driver.rs, src/cli/exit.rs); the suites read the new keys
- [x] B9: `init` splits its failure into `error` and `sources`, its endpoints under `opened` (src/cli/init.rs)
- [x] Tests: `a_listed_conflict_prints_its_id_as_a_string`, `a_partial_capability_prints_as_a_row`, `a_report_lists_its_patches_as_arrays_and_names_no_account`, `a_failed_init_carries_its_causes_apart`
- [x] CHANGELOG, MIGRATION.md
- [x] Fold into cairn/spec/sync.md, log, archive
