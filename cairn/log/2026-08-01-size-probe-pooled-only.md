---
cairn: log
change: size-probe-pooled-only
landed: 2026-08-01
---

# Largest-first size probe runs only on the pooled fetch path

Trivial follow-up to the fetch-path work: `fetch_full` ran the `sizes()` probe
(a `UID FETCH <set> (RFC822.SIZE)` round trip) unconditionally, then checked
whether the batch was worth a connection pool. For a trivial batch (≤1 connection,
or a single message) that streams serially, largest-first ordering does nothing,
so the probe was a wasted round trip. Moved the probe (and the sort) after the
`target <= 1` check, so it runs only when several connections actually race.
Behaviour is otherwise unchanged; bodies fetch identically. Trivial fix, no
proposal; spec clause folded into "Hydration may run concurrently, largest-first".
