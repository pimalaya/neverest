---
cairn: log
change: one-warning-per-parked-action
landed: 2026-08-28
---

# One parked row, one warning

`drain_queues` ended by reading `parked_actions` into the report. That read is
of the whole store, the queue recording no source, while the drain around it
runs once per source, so a posteo account with an IMAP, a CardDAV and a CalDAV
source reported its one parked row three times, identical down to the `#1`.

**Split** (`offline/driver.rs`): `drain_queues` now drains and reports what it
applied, and `report_parked` reads the parked rows, once, from `run` after the
source loop. It runs in a dry run too, a parked row being a fact about the store
rather than something the run did.

Covered by `a_parked_action_is_reported_once_however_many_sources_drained`:
three drains over one store holding one unappliable action, one entry in the
report. On the old code the same test reports three.

Unrelated, found while verifying: `the_sweep_collects_the_bodies_the_purge_released`
was flaky, failing about one run in four. `purge-after = "0s"` purges what was
retained *strictly* before the cutoff, and the cutoff carries milliseconds, so an
item dropped and swept inside one millisecond is not old enough to go and the
sweep reports nothing purged. The production semantics are right (real delays are
days) and the test was racing them, so the test now ages the item by 5 ms. 25
runs of it and 8 of the whole suite, all green.

Spec updated: `sync` (MODIFIED: "Neverest is the store's sole owner and drains
the queue first", the parked rows now stated to surface once per run and read
after every source has drained).
