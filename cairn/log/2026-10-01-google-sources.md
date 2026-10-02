---
cairn: log
change: google-sources
landed: 2026-10-01
---

# Google contacts and calendars are native sources

Two backends: `gpeople` (src/gpeople/client.rs) syncs Google contacts through the People API and `gcal` (src/gcal/client.rs) Google calendars through the Calendar API, both over the projections io-gpeople and io-gcal now carry, both bearer-only through a shared `GoogleConfig`, both in the default feature set.

People has one collection, `contacts`, every connection. Enumeration is the connections listing with a sync token, the person's etag the revision; updates mirror Cardamum's, the etag in the body and the foreign `clientData` entries merged back, and a create whose UID stash People dropped is deleted again and refused.

Calendar's collections are the user's calendar list. A recurring series and its modified instances, which Google returns as events of their own, are one item under the series id, the way a CalDAV resource holds a series; its revision joins their etags, so an edit to one instance moves it, and a body read fetches the series by `iCalUID`. A create imports an object carrying a UID, keeping it as the `iCalUID`. Writes are checked against the joined revision, then guarded by the series event's etag; a locally modified instance does not push yet, which Calendula shares.

Both resume from a sync token, a resumed listing naming removals and a full one reporting them by absence, an expired token (HTTP 410) restarting a full listing. Kind gates now cover `gpeople` and `gcal` too, so each builds alone.

Not done: wizard entries, instance push, an end-to-end run against a Google account. io-gpeople's UID stash is unreleased, so Cargo.toml patches it to the local checkout.

Capabilities moved: **sync** (Google contacts and calendars are native sources; sources are remote backends only, every remote backend is a cargo feature, the sugar keys, modified).
