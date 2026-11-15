---
cairn: delta
change: a-band-lists-from-its-floor
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A mail sync lists within a scope on the `Date`
The second paragraph gains: a provider's date filter that reads another date than the `Date` header (Gmail's arrival, Graph's `sentDateTime`, Exchange's submission) SHALL narrow a round by the scope's floor alone, never by a band's ceiling, the page keeping what its `Date` puts in the band; the scenario *A band keeps a message dated in it and received after it* is added.

### Requirement: A provider narrows a scope with a margin
Gmail narrows by `after:` two days below the floor and Graph by `sentDateTime` from the floor, neither by a band's ceiling; IMAP keeps `SENTBEFORE` two days above a ceiling. Graph's `sentDateTime` is no longer said to be its reading of the `Date` header.

## REMOVED Requirements
