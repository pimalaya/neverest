---
cairn: change
change: the-sidecar-is-written-whole
---

# Delta

## ADDED Requirements

### Requirement: The sidecar is replaced whole
neverest.json SHALL be written to a temporary file in the store directory, synced, and renamed over the previous one, so a run killed while saving it leaves the previous sidecar or the new one, never a torn file the next run refuses.

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
