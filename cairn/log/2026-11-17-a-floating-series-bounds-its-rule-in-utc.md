---
cairn: log
change: a-floating-series-bounds-its-rule-in-utc
date: 2026-11-17
---

# A floating series bounds its rule in UTC

Finding NEV-19, part d, of the CLI family analysis of 2026-11-15.

## What landed

- Anchoring a floating series for Google Calendar or Graph (`calendar::anchor_floating`) now rewrites the floating `UNTIL` of its `RRULE` and `EXRULE` as the UTC instant its wall time names in the anchoring zone, UTC when none. Before, the start gained a `TZID` and the bound stayed floating, a rule RFC 5545 §3.3.10 forbids, which a provider refuses or reads in another zone.
- Not checked live: what Google and Graph did with the old form.

## Spec

sync.md, "A floating time is anchored before Google or Graph takes it".
