---
cairn: change
id: one-warning-per-parked-action
status: landed
created: 2026-08-28
---

# A parked action is reported once, not once per source

## Why

A queue action the drain finds permanently unappliable parks, and every run
re-reports it until it is repaired. That is right: a parked row is work a
frontend asked for and neverest could not do, and silence would lose it.

It was reported once per source. The parked rows were read where the drain runs,
and the drain runs per source, while `parked_actions` is a read of the whole
store: it takes no source and the queue records none. So an account whose mail,
contacts and calendar each drained showed the same row three times.

```
Warnings (3):
 - parked queue action #1 (set-flags in imap/INBOX from himalaya): seq 6951 projects no placement
 - parked queue action #1 (set-flags in imap/INBOX from himalaya): seq 6951 projects no placement
 - parked queue action #1 (set-flags in imap/INBOX from himalaya): seq 6951 projects no placement
```

Three warnings reads as three problems. It is one row, `#1` in all three lines,
and the id was the only thing saying so.

## What

- `drain_queues` drains and reports what it applied, and nothing else.
- `report_parked` reads the parked rows once for the run, after every source has
  drained, in a dry run as much as a real one.

## Not in scope

**Why the row parked.** `seq 6951 projects no placement` is a separate question,
answered where the drain projects an action onto a source that holds no binding
for it.
