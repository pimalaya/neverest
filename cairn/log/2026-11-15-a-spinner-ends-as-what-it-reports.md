---
cairn: log
change: a-spinner-ends-as-what-it-reports
date: 2026-11-15
---

# A spinner ends as what it reports

Ledger lines N13 and N19 of the CLI alignment round of 2026-11-15, from live testing.

## What landed

- Cargo.lock: pimalaya-cli 0.5.0 to 0.5.2, no other package moved.
- src/cli/check.rs: a source or an SMTP channel that did not answer ends its spinner with `failure`.
- tests/exit.rs: `a_failed_check_prints_its_report_and_exits_3` reads `✗ Source imap failed` and no escape on stderr; it failed with the `success` call.

## Capabilities moved

- [sync](../spec/sync.md): *A spinner ends as what it reports* added.
