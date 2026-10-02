---
cairn: log
change: a-graph-source-declares-its-domain
landed: 2026-10-01
---

# A Graph source declares its domain

Graph syncs contacts. A source declaring `msgraph-contacts` gets the user's contact folders as address books, keyed by folder id and named by display name, the default Contacts folder as `contacts`; `msgraph` stays mail. One `GraphClient` serves both through a `GraphKind`, the way one DAV adapter serves CardDAV and CalDAV, and the contacts half lives in `src/msgraph/client/contacts.rs`.

The projection is io-msgraph's `vcard` feature, moved there from Cardamum, and the vCard `UID` now rides its stash extended property, so a card written through neverest keeps the identity it arrived with and matches its copy on a DAV endpoint. A contact Graph created reads back under a UID minted from its Graph id, stored the next time it is written. A create is read back with its stash and refused, the contact deleted again, when the UID did not survive.

Three choices differ from the proposal. Bodies are read per contact with the stash expanded rather than projected from the delta row: whether the contacts delta honours `$expand` is undocumented, and a row without the stash would read a card under a minted identity. Graph has no conditional write for contacts, so an `If-Match` is checked by reading the contact's `changeKey` first. Collections are keyed by folder id rather than by `Parent/Child` names, following `a-collection-is-named-not-only-addressed`.

A contacts source carries no mail: `carries_mail` refuses an `smtp` table on it, `sends_natively` stays a mail-source property, and the driver only takes a Graph client of the mail kind as the native sender. `Kind::Vcard` and `Kind::Ical` now compile under `msgraph` as well as `dav`, so a Graph-only build syncs cards, and `msgraph` is back in the default feature set.

Not done: the wizard and ortie's README do not name the `Contacts.ReadWrite` scope yet, and no end-to-end run against a real tenant exists. Until io-msgraph releases its UID stash, Cargo.toml patches it to the local checkout.

Capabilities moved: **sync** (a Graph contacts source, a Graph contact keeps its identity, the projection belongs to the protocol crate; Microsoft Graph is a first-class source, sources are remote backends only, every remote backend is a cargo feature, and the send channel rules, modified).
