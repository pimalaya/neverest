---
cairn: delta
change: a-graph-source-declares-its-domain
---

# Delta

## ADDED Requirements

### Requirement: A Graph contacts source syncs the address book
A source MAY declare an `msgraph-contacts` backend, whose items are `text/vcard` cards and whose collections are the folders under the user's contact folders, listed two levels deep and named `Parent/Child` as the mail folders are. The default Contacts folder SHALL be addressed by the sentinel id Graph omits from the request path.

It SHALL accept the same `user-id`, `tls`, `alpn` and `auth` fields as an `msgraph` source, and SHALL carry no send channel: submission is a mail capability, so an `smtp` table on it is refused before any connection is made, and it SHALL NOT be picked as the account's native sender however the sources are ordered.

Enumeration SHALL be the contacts delta query, carrying the `@odata.deltaLink` as the engine's opaque checkpoint under the same rules mail follows: HTTP 410 restarts a fresh full round, any other failure surfaces, and handle identity survives a reset. The delta query SHALL carry the `$expand` clause that returns the extended property the identity lives in, Graph omitting extended properties otherwise.

Contacts are **mutable content**: `change_key` SHALL be reported as the revision on every enumerate and fetch, an update SHALL be a conditional write against it, and a refused write SHALL be reported as rejected so the engine re-merges. Unlike mail, a Graph contacts source pushes creates and updates as well as deletes.

The body SHALL be projected from the delta row already held, never re-fetched: the row carries the whole contact, so the `Full` tier costs no round trip even though the kind resolves there.

Flags SHALL be reported known-empty, Graph contacts having no flag concept.

#### Scenario: An address book syncs, follows a server edit and pushes one back
- GIVEN a Graph tenant holding two contacts in the default Contacts folder
- WHEN the account is synced, one contact is edited on the server, another edited in the store, and it is synced again
- THEN the store holds both cards keyed by their vCard `UID`, follows the server edit, and the store edit reaches Graph as a conditional update

### Requirement: A Graph contact keeps the identity it arrived with
A Graph contact carries no vCard `UID` of its own, so the identity SHALL be stored on the contact as a single-valued extended property and read back from it, and that stored value SHALL be the card's `UID` and therefore its link id.

A contact carrying no stored identity is one Graph itself created. Only then SHALL a `UID` be minted, from the Graph id, and it SHALL be stored on the contact in the same write, so the mint happens once and never again.

A `UID` SHALL NOT be minted from the Graph id on read, and a card's identity SHALL NOT fall back to the content hash. Minting on read gives one person a Graph-local identity on one side and its real `UID` on the other, so a Graph source paired with a CardDAV one matches nothing and duplicates the whole address book every run; hashing the content repoints the identity on every field edit, so an edit reads as a delete and an add. Both defeat `One identity is one item across an account's endpoints`, which SHALL hold across a Graph endpoint and a DAV one.

A write whose stored identity the server did not accept SHALL be refused, never completed with a minted one.

#### Scenario: A card crosses from CardDAV to Graph and back unchanged
- GIVEN a card with `UID:urn:uuid:…` on a CardDAV endpoint and a Graph contacts endpoint in the same account
- WHEN the account is synced, then synced again
- THEN one item exists under that `UID`, the Graph contact carries it as its stored identity, and the second run finds nothing to do

### Requirement: The Graph projection belongs to the protocol crate
The mapping between a Graph contact and a vCard SHALL live in io-msgraph, beside the resource it projects, and SHALL NOT be written a second time in a consumer. It projects both ways and produces a delta against a base card, and every line the vCard carries that Graph has no slot for SHALL survive on the server through the stash remainder rather than being dropped or folded into another slot.

Two copies of a projection drift silently and lose data at the seam between them, which is the failure this rules out rather than mitigates.

## MODIFIED Requirements

### Requirement: Microsoft Graph is a first-class source
An `msgraph` source SHALL open protocol-direct over io-msgraph (never through a frozen aggregator). Graph carries several PIM domains behind one protocol, one host and one credential, and which domain a source syncs SHALL fall out of its backend as it does everywhere else: `msgraph` is mail, `msgraph-contacts` is the address book. One adapter SHALL serve both, parameterised by which it speaks, as one adapter serves both DAV protocols. A `kind` field on the configuration SHALL NOT be introduced for this or any other backend.

Graph carries **no calendar**. io-msgraph implements no events resource, and the documentation SHALL say so outright rather than leave a reader to infer it from a missing key.

For **mail**: folders listed two levels deep (`Parent/Child` naming), enumeration through the messages delta query carrying the `@odata.deltaLink` as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh full round; any other failure surfaces), the `Meta` tier served from the cached delta rows (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME content streamed into the blob store. Flags map to the IANA wire spellings (`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`). Push scope is honest: flag changes push through `message_update` and deletes through `message_delete`; appends, moves and content updates are rejected (pull-only) and documented.

Auth SHALL be a bearer access token only, resolved through the standard secret-command idiom (`auth.token.raw` / `auth.token.command`) once per run with every other credential; neverest SHALL NOT run any OAuth flow itself (no device sign-in, no client credentials, no token persistence): acquiring and refreshing the token is delegated to an external command, typically ortie. The token SHALL carry the scopes the declared domains need, and the wizard SHALL name them per domain rather than let the first run fail on a bare 403. No token is ever logged.

### Requirement: Sources are remote backends only
A sync source SHALL be a remote backend: IMAP and Microsoft Graph for `message/rfc822`, CardDAV and Microsoft Graph for `text/vcard`, CalDAV for `text/calendar` (JMAP and Gmail as their backends land). Local file backends (m2dir, maildir, vdir) SHALL NOT be sync sources: the pimdir store is the local replica, so a local file store is redundant as a source and belongs on the import/export path, which neverest documents rather than syncing directly.

### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `msgraph-contacts`) SHALL be sugar for `sources.<key>.<key>`, the source taking the key as its name. The sugar SHALL produce a configuration indistinguishable from the expanded form, source id included, so expanding it by hand is a no-op on the store.

A key names a protocol wherever a protocol carries one domain, and a protocol-and-domain pair where it carries several: `carddav` and `caldav` are two keys over one adapter, and so are `msgraph` and `msgraph-contacts`. The key is what the kind falls out of, so it SHALL be distinct per kind even when the transport is shared.

Declaring the same key both directly and under `sources` SHALL be a configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

### Requirement: A send channel belongs to at most one source
At most one source per account SHALL declare `smtp`. Two or more SHALL be a configuration error, reported at load, rather than a silent tiebreak on source order. A source that sends by itself (Microsoft Graph mail, through `sendMail`) needs none.

Sending natively is a property of the source's **kind**, not of its transport: a Graph source of any other kind SHALL NOT be offered as the account's sender, and SHALL refuse an `smtp` table like any other non-mail source, so an account carrying two Graph sources still has at most one channel.

The account root MAY carry the `smtp` table when its mail backend is the direct-backend sugar, in which case it completes that one source; with no direct mail backend, or several, it SHALL be refused.

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend, `msgraph` for the Microsoft Graph backends, `dav` for the CardDAV and CalDAV backends, `smtp` for the SMTP submission channel.

Backends sharing one adapter and one dependency SHALL share one feature rather than take one each, separate features gating nothing that is separately compiled: `dav` covers CardDAV and CalDAV, `msgraph` covers Graph mail and Graph contacts. A feature that merely aliases another is not introduced for an older spelling.

All of them SHALL ship in the default feature set, `msgraph` included again: it was held out under `msgraph-waits-for-its-domains` because a Graph account meant mail and nothing said so, and declaring the domain is what lifts that.

A missing backend SHALL surface at runtime, never at build time: every feature combination compiles, the configuration surface stays whole (every source config still parses), and an unavailable backend fails when the source is *opened*, as the JMAP and Gmail sources already do. A build with neither `smtp` nor `msgraph` has no send channel and SHALL warn rather than perform a submit intent. Each optional backend crate SHALL take its TLS provider from neverest's own `native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than pinning one.
