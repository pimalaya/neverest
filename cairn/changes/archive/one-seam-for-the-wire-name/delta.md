---
cairn: delta
change: one-seam-for-the-wire-name
---

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the
source's media type, the source's `collection.namespace`, and the collection
name the backend enumerates. The bare collection name SHALL NOT be the id,
because a CardDAV address book and a mailbox may carry the same name in one
store. The id is spelled `<namespace>/<name>` with the kind on the collection
row, and the namespace prefix SHALL be stripped back off before any call reaches
a server, at one seam, so a backend only ever sees the name it gave. A report
SHALL name a collection the way its server does, not the way the store keys it.

Every wire call SHALL pass through that seam, including the ones the solo sync's
body-hydration pool makes on its own connections rather than through the remote,
and including a collection named as an argument rather than as the target: a
move destination is a hub id like the collection it leaves. A cache keyed by
collection SHALL keep the hub id as its key, the seam being the wire call and
not the plan.

## REMOVED Requirements

None.
