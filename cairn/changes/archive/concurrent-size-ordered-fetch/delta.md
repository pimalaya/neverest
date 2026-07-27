---
cairn: delta
change: concurrent-size-ordered-fetch
---

## ADDED Requirements

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration MAY be serviced by a bounded pool of connection-owning
workers running whole-message jobs, scheduled largest-first by the enumerated
member size, so a heavy message overlaps the light ones instead of stalling the
tail. The pool size SHALL NOT exceed the backend's connection limit. Body bytes
SHALL stream lock-free into the blob store; only the index commit serialises on
the single-writer store.

## MODIFIED Requirements

(none)

## REMOVED Requirements

(none)
