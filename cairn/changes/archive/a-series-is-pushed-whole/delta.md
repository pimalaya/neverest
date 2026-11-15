---
cairn: delta
change: a-series-is-pushed-whole
---

# Delta

## ADDED Requirements

### Requirement: A series is pushed whole or not at all
A Google or Graph write carries the master, then each occurrence the document sets apart through its instance (Graph: `EXDATE` and cancelled overrides deleted, overrides patched; Google: overrides updated, `EXDATE`s in the recurrence), or is refused coded `unsupported`; a create whose occurrences do not all land is deleted again; `calendar.occurrence.update` is declared `partial` on both. Three scenarios.

## MODIFIED Requirements

### Requirement: Microsoft Graph is a first-class source
"Only the series master is written" replaced by a pointer to the new requirement.

### Requirement: Google contacts and calendars are native sources
"Only the series event SHALL be written" replaced by a pointer to the new requirement.

## REMOVED Requirements
