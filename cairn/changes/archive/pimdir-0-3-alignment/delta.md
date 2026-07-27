---
cairn: change
change: pimdir-0-3-alignment
---

# Delta

## ADDED Requirements

### Requirement: Every item carries a per-kind sort key
The sync SHALL write a `sort_key` beside the `meta` of every item it summarises
(pimdir SPEC §9.3), derived by the same per-kind seam and never parsed back out
of the summary by the store. `message/rfc822` SHALL carry the `Date:` header
normalised to RFC 3339 in UTC at seconds precision, so byte order is
chronological order whatever offset the sender wrote; `text/vcard` SHALL carry
the display name (`FN`) casefolded and trimmed. A kind resolving at two tiers
SHALL derive the byte-identical key at both, on the same terms as its link id: a
key that moved when the body arrived would re-sort the item on hydration.
Content carrying nothing to derive from SHALL keep the empty key, which the
store reads as unknown.

### Requirement: Bodies are named by the store's own hash
The content hash naming an object SHALL come from the store handle
(`PimdirStore::blobs`), which computes the algorithm `store_meta.hash_algo`
records (pimdir SPEC §5), never from a digest neverest defines. A consumer
picking its own names bodies where no other reader of the same store looks, and
it fails silently, as a dedup that never dedups.

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each
collection it writes is grouped under that account (pimdir SPEC §9.2). Two
hand-written accounts may share one `store.root`, and a reader of such a store
SHALL be able to tell whose collection is whose without inferring it from the
collection naming.

### Requirement: A store the format outgrew is refused with its remedy
A store an earlier draft of the pimdir format wrote cannot be migrated in place.
Opening one SHALL fail naming `sync --reset` for the account, the command that
drops the replica and resyncs it, rather than surfacing the raw refusal: the
store is a derived cache, so recreating it costs a resync and loses nothing but
un-pushed local mutation.

### Requirement: A DAV connection survives a server that closes it
A DAV side SHALL reopen its connection and run the exchange again when the
server closed it between requests (an HTTP/1.0 answer, a `Connection: close`),
carrying the discovery it already paid for over to the new connection. Only an
end-of-stream or reset failure SHALL be retried, being the shape of a request
the server never read, so a create or a delete is never replayed against a
server that acted on it.

## MODIFIED Requirements

### Requirement: A local sync retains every body
A one-side sync SHALL hydrate every synced item to `Full` (fetch its body into the
store), because the store is the app's offline copy — distinct from the two-source
path, which hydrates only bodies about to cross. It SHALL pull before pushing so an
edit the app staged locally stays pending and is reported (and pushed) rather than
swallowed, and it SHALL open the store as the one side's source so an app writing
as that same source stages edits the sync pushes.

The item a hydration pass picks up SHALL be selected by the **absence of a
stored body**, not by its detail level. A remote content change drops the stale
body while the hub keeps the level the item had reached, so a pass keyed on the
level would leave an edited item bodiless for good.

### Requirement: A probed item is raised to the tier its kind resolves at
Every freshly probed placement SHALL be raised to the tier its kind resolves its
link id and summary at: `Meta` where the backend offers a cheap server-side
summary (mail's IMAP `ENVELOPE`), `Full` where only the body carries the
identity. Raising a DAV item to `Meta` asks its backend for a summary tier it
does not have, which fails the scan of every DAV collection.

## REMOVED Requirements
