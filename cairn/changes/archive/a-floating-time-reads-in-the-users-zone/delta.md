---
cairn: delta
change: a-floating-time-reads-in-the-users-zone
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A floating time is anchored before Google or Graph takes it
On Graph, and on Google when the calendar states no zone, a floating time is anchored in the user's zone, the one the search index states (`TZ`, else the system's, UTC when it has no IANA name), where it was anchored in UTC. Scenario *A floating meeting queued on a Graph calendar* added.

## REMOVED Requirements
