---
cairn: log
change: a-failed-run-says-so
date: 2026-11-18
---

# A failed run says so

Found by the live run of 2026-11-17 (N13, offline): the text summary read "Account fastmail partially synchronized: 3 hunks, 3 errors" where the three hunks were the failed listings and nothing was synchronized.

## What landed

- src/sync/report.rs: a run with errors counts in its summary the hunks it applied, not the ones that failed. With none applied it reads "Account <name> not synchronized: <n> errors". The JSON is unchanged.
- config.sample.toml: the Graph block says a domain syncs only when named, `true` or any key under it, so `#msgraph.contact = true` no longer reads like a default.
- Test: `a_run_whose_every_hunk_failed_is_not_synchronized` (src/sync/driver/tests.rs), failing on the code before.

## Spec

No requirement quotes the summary line; no spec change.
