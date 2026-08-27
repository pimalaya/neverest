---
cairn: change
id: a-scan-failure-is-reported
status: landed
created: 2026-08-28
---

# A collection that failed to scan is reported, with the error that caused it

## Why

An account reported `already in sync` for a source whose only collection had
failed to enumerate. Two lines conspired, and both hide evidence rather than
produce it.

**A failed scan is a warning and nothing else.** `phase1_spine` catches a
collection whose spine failed, logs it and moves on, so the report carries
nothing about it. The run then prints `already in sync`, which is the one
sentence that cannot be true: the sync did not decide the collection was
unchanged, it failed to look. An account can be broken indefinitely and every
run say it is fine, which is worse than the failure it is hiding.

**The engine's error wrappers truncate the chain.** `Remote enumerate error:
{err}` renders an `anyhow` error with `Display`, which prints the outermost
context and drops every source under it. So a server's HTTP status and response
body, which the backend keeps verbatim precisely so a caller can read them,
never reach the operator. Diagnosing the failure above meant reading a TLS trace
log to find a status the error already held.

## What

- `phase1_spine` records a failed collection as a `PatchEntry` carrying its
  error, so the run reports it and exits non-zero instead of claiming to be in
  sync. The warning stays, for the log.
- The enumerate, fetch and push wrappers render with `{err:#}`, so the whole
  chain, down to the backend's status and body summary, reaches the report.

## Not in scope

**Why that particular server refused.** This change makes the refusal legible;
it does not decide whose fault it is. A backend that should fall back to another
enumeration, or a request this crate builds wrongly, is a separate change and
needs the evidence this one surfaces.
