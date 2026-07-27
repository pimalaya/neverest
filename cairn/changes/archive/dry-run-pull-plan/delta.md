---
cairn: change
change: dry-run-pull-plan
---

# Delta

## ADDED Requirements

### Requirement: The report shows the one-source pull plan
A one-source sync SHALL report its pull plan — each not-yet-`Full`, non-tombstone
item it would download into the store — as `Fetch` hunks, in both a dry run (which
stops there) and a real run (which then hydrates them). So `sync --dry-run` shows
what a fresh sync would download (rather than "already in sync"), and a real run's
report reflects the download, its main work.

## MODIFIED Requirements

## REMOVED Requirements
