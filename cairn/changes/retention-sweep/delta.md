---
cairn: change
change: retention-sweep
---

## ADDED Requirements

### Requirement: A run reclaims retained items on a schedule
The store retains an item rather than deleting it when its last binding
vanishes, so reclaiming is the client's schedule. An account SHALL configure it
as `store.purge-after`, a human duration (one integer plus `s`/`m`/`h`/`d`/`w`).
**Unset SHALL mean never purge**; `"0"` SHALL purge immediately, reproducing a
terminal delete. There SHALL be no boolean beside it: the delay is the switch.

A sync run SHALL sweep **after** the sync and before the report is finalised, on
both the two-side and the single-source paths, never in a dry run, purging every
retained item whose `retained_at` precedes `now - purge-after` (RFC 3339, the
shape the store stamps). Sweeping after the sync means an item this run retired
starts its delay now. The sweep SHALL warn rather than fail the run, as the send
channel does, and `sync --no-purge` SHALL skip it. The report SHALL carry what
was reclaimed (items and bytes) in both the text and `--json` output.

A read-only remote side (`<side>.<backend>.item.delete = false`,
`collection.delete = false`) with no purge delay is therefore a backup: a remote
expunge retires the local row without losing the item or its body.

## MODIFIED Requirements

## REMOVED Requirements
