---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# The errors a report counts are the ones it lists

Task N12 of the-live-night-findings. On the live night of 2026-11-23, a drain whose send Stalwart refused for a rate limit (452 4.4.5) printed `Errors (1):` and no line under it: the count took every error, while the list held only failed hunks and store steps, a send or an intent being listed under its own section.

The `Errors` list of the text report (src/sync/report.rs) now takes every error it counts. Before:

```
Submissions (1):
 - hi not submitted, retried next run: 452 4.4.5 rate limit

Errors (1):

Account stalwart not synchronized: 1 errors
```

After:

```
Submissions (1):
 - hi not submitted, retried next run: 452 4.4.5 rate limit

Errors (1):
 - hi not submitted, retried next run: 452 4.4.5 rate limit

Account stalwart not synchronized: 1 errors
```

Capability moved: sync.md, "The report accounts for every write the run made".

## Checked

- src/cli/exit.rs `the_errors_counted_are_listed`: fails before (the empty list above), passes after.
