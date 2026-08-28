---
cairn: log
change: calendar-over-caldav
landed: 2026-08-28
---

# Calendar landed as a flavour of the backend contacts already had

The third PIM domain turned out to be almost no new code, which is the point worth recording. io-webdav's RFC 4791 surface is a method-for-method mirror of the RFC 6352 one, io-pim-discovery finds a CalDAV endpoint through the same RFC 6764 mechanism it finds a CardDAV one, and io-pimdir's `conventions::calendar` already derives the link id, the Annex A.3 summary and the sort key. The engine, the store and the client seam are kind-neutral and needed nothing.

So the change is mostly a subtraction. `src/carddav/` became `src/dav/` and the client carries a `DavKind`: the two protocols differ in the home set they discover, the collection they list and the extension a new resource is named with, and in nothing else. Writing the adapter twice would have meant maintaining 580 lines of enumeration, multiget, conditional writes and the HTTP/1.0 reconnect repair in two copies, and the CardDAV copy is the one debugged against real servers. The `carddav` cargo feature became `dav` for the same reason: one dependency, one adapter, one discovery mechanism, so two features would have gated nothing separately compiled. It is unreleased, so the rename costs nothing.

The kind delegates outright, which the card kind beside it still does not. The two reasons a scanner stayed local do not apply here: io-pimdir reads a calendar's summary fields verbatim, which is how Annex A.3 spells them and what a reader wants, and it resolves the sort key through the `VTIMEZONE` the resource itself carries. That resolution is the one genuinely hard part, and the one two writers of a store must not answer differently, so a second implementation could only drift. The single thing added on top is restating `meta.size` when the streamed prefix was capped, since io-pimdir sizes from the bytes it was handed and only the stream knows the whole resource's length.

The item is the calendar object resource, never the component, so a recurring series and its `RECURRENCE-ID` overrides are one item under one link id and an override is a body edit. That is RFC 4791 §4.1 and it is what keeps `(collection, link_id)` exactly the uniqueness CalDAV itself enforces.

Verified end to end against Radicale, on the same four steps as contacts: two events land keyed by their `UID` and ordered by their start, a server-side edit is followed, a server-side delete is retained rather than lost, and the run after it is quiescent. The CardDAV run passes against the same server, which is what makes the shared adapter safe to have.

Writing the CalDAV twin turned up two stale assertions in the CardDAV one, both of which say it had not been run since the changes that broke them. It looked for `uid:card-1`, which was the link id before `adopt-the-format-conventions` made it the bare `UID`; and it listed the store's `contacts` collection, where pimdir groups a collection under the id of the source that syncs it, so the key is `carddav/contacts`. Both are fixed. An `--ignored` test that needs a container is a test nobody runs by accident, which is the price of covering a real server at all, but it is worth knowing that price is paid in silence.

Capabilities moved: sync, two new requirements (the CalDAV source, the one shared adapter) and three modified (the feature set, the per-kind derivations, the conventions delegation).
