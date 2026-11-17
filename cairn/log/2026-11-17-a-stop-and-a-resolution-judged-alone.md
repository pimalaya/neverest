---
cairn: log
change: a-stop-and-a-resolution-judged-alone
date: 2026-11-17
---

# A stop cuts a throttled wait, a resolution is judged alone

Two parts of finding NEV-12 of the CLI family analysis of 2026-11-15.

## What landed

- src/throttle.rs: `sleep_unless_stopped` waits in 200 ms steps and gives the request up on a stop signal; `Throttle::call` and `pace` use it. A throttled request kept sleeping and retrying after the first SIGINT, up to five waits of 120 s. Test tests/queue.rs `a_stop_signal_cuts_a_throttled_wait_short` (a 503 stating 100 s): the old code sat out more than 5 s after the signal, the new one stops at once, exit 4. Unix only (it sends SIGINT with `kill`).
- src/conflict.rs: `Conflict::apply` reads its own row's status after the drain (`action_status`) where it read the store-wide counters, so another app's row parking in the same drain failed a resolution that was applied. Test `a_resolution_is_judged_by_its_own_row`, failing on the old code.
- Left of NEV-12: the calendar intents' in-flight mark, the auto-merge draining mid-run, Gmail `remove` archiving through the store.
- Trivial fixes: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *A signal stops a sync cleanly* and *Deciding is a command, never a run* amended.
