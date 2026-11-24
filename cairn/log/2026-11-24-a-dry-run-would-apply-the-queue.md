---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A dry run would apply the queue

Task N9 of the-live-night-findings. On the live night of 2026-11-23, a dry run over a store holding queued rows printed `applied N queued action(s)`: its drain ran on the replica, the real queue untouched, but the line read as done.

The text report (src/sync/report.rs) now prints the queue of a dry run as what it would do. Before:

```
Queue (1):
 - applied 2 queued action(s) in stalwart/imap/INBOX
```

After:

```
Queue (1):
 - would apply 2 queued action(s) in stalwart/imap/INBOX
```

Capability moved: sync.md, "A dry run works on a replica that shares the bodies".

Seen while testing it, left as it is: a dry run whose only change is the queue ends on "Account stalwart is already in sync", the drained rows counting as no hunk.

## Checked

- src/cli/exit.rs `a_dry_run_would_apply_the_queue`: fails before (the `applied` line above), passes after.
