---
cairn: log
change: a-check-reports-every-endpoint
date: 2026-11-03
---

# A check reports every endpoint

`check_source` (src/cli/check.rs) no longer returns a `Result`: a credential that does not resolve, an endpoint that does not open or list, and an SMTP channel that does not authenticate each land in their row. A row is `{source, backend, ok, code, error, collections, capabilities, smtp}`, `smtp` being a `ChannelCheck` `{backend, ok, code, error}` or `null`. Rows are sorted by name. A failed row or channel prints the report and returns `PartialOutcome`, which `ErrorReport::eval_coded` turns into exit 2.

Capabilities moved: sync (cairn/spec/sync.md: "A check reports every endpoint" added; "A check opens the send channel too" and "A data command describes what it prints" modified). Guarded by `a_check_names_every_endpoint_that_did_not_answer` (tests/check.rs), which runs without a server and fails on the code before this change (exit 1, the failure object of the first endpoint). `cargo test --all-features` 486 unit tests passed, clippy clean.
