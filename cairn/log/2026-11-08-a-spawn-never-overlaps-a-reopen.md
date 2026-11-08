---
cairn: log
change: a-spawn-never-overlaps-a-reopen
date: 2026-11-08
---

# A test's spawn never overlaps another's reopen

The Releases run of 7d48121 failed its x86_64-windows job in the native neverest the cross build runs for its completions and man pages: `a_store_written_under_the_merger_sends_the_decision_back_for_another_look` and `a_reset_takes_the_search_index_with_the_store` were refused their store as `Owned`, each owning it again right after releasing it. The same derivation built clean elsewhere, so a race, not Windows.

The cause: a child another thread spawns gets a copy of every descriptor of the process until its exec, owner.lock's among them, and the flock lives on until the last copy closes. A store owned again in that window is refused, as STORAGE §8 says an owner must be (fail immediately, never wait). Proven with a throwaway test: one thread spawning `true` in a loop, another reopening a store, 2265 of 5000 reopens refused. The merger tests spawn, beside tests that reopen a store.

## What landed

- `conflict::merger::SPAWNS`, a test-only `RwLock`: the three merger tests and the conflict test whose merger runs hold it exclusively, the four tests going through `reset_replica` (which releases the store and owns it again) hold it shared.
- Proven: throwaway tests spawning `true` 3000 times beside 3000 `reset_replica` runs fail within a second without the lock ("owned by another process") and pass with it. The suite passed three runs in a row (506 tests).
- No program change: neverest resolves every secret before connecting, and runs the merger with the store released and no other thread spawning, so no run of the program meets the race.

## Left

- Any other test releasing a store and owning it again is exposed the same way while a merger test spawns; one that fails so takes the lock shared.

No capability moved.
