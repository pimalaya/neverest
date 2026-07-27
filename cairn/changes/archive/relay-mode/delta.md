---
cairn: change
change: relay-mode
---

# Delta

## ADDED Requirements

### Requirement: A two-source sync may relay instead of retain
A two-side sync SHALL support a `store.retention` mode. Under `Retain` it keeps
every body in the store (the current behaviour). Under `Relay` a cross-copy body
SHALL be streamed directly from its holding side to the other through a bounded
in-memory pipe — the store keeping only the spine (the item is never hydrated, no
object blob at rest; the target's next enumerate binds the relayed message). The
target APPEND length SHALL come from the item's `v:1` meta `size`, so no body is
buffered to discover it. Relay is IMAP-first: it is the **default for an IMAP↔IMAP
pairing** and unavailable otherwise (any non-IMAP side retains, and an explicit
`relay` on such a pairing falls back to retain with a warning). Relay trades away
dedup, cheap retry and resumability (a failed copy re-fetches from the source), so
it is for the pure pass-through mirror; retain stays the default wherever a local
reader exists (every one-side/local sync).

## MODIFIED Requirements

## REMOVED Requirements
