---
cairn: delta
change: a-collection-is-named-not-only-addressed
---

# Delta

## ADDED Requirements

### Requirement: A collection is keyed by its backend id and named by its display name
Every hub collection id, every wire call and the account's collection filter SHALL be built from the backend's own id: the mailbox name on IMAP and on Graph, the path segment on DAV. A display name SHALL NOT address a collection, being optional, mutable and free to collide.

The store SHALL still carry what the collection is called: `set_collection_name` runs beside every `ensure_collection` with the name without the namespace the hub id carries. A DAV source SHALL take it from `DAV:displayname`, falling back to the path segment when it is blank or absent; every other source's id is already its name. Where two sources meet, the first in declared order with a non-blank name wins. Naming SHALL NOT fail a run: nothing keys on the column, so a refused write is logged and the sync continues.

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
