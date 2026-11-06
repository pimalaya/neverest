---
cairn: tasks
change: a-check-reports-every-endpoint
---

# Tasks

- [x] `check_source` folds every failure into its row; `ChannelCheck` for the SMTP channel
- [x] Rows sorted; exit 2 through `PartialOutcome` when one failed
- [x] Test `a_check_names_every_endpoint_that_did_not_answer`, without any server; it fails on the old code (exit 1, failure object)
- [x] CHANGELOG
- [x] Fold into cairn/spec/sync.md; log; land
