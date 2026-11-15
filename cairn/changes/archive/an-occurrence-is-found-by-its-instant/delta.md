---
cairn: delta
change: an-occurrence-is-found-by-its-instant
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A series is pushed whole or not at all
An occurrence's instance is found by its original start compared as an instant, a value read in its `TZID`; a wall time of no zone known matches as the provider writes it or in the series' zone. Graph patches an override against its series carried to the occurrence, so what it leaves out stays as the series has it; an exception Graph holds is patched against itself. Scenarios *An override in a zoned series reaches Google* and *An override renaming one occurrence reaches Graph* added.

## REMOVED Requirements
