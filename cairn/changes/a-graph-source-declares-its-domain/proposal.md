---
cairn: change
id: a-graph-source-declares-its-domain
status: active
created: 2026-09-07
---

# A Graph source declares its domain

## Why

Microsoft Graph is the only backend neverest speaks that carries all three PIM domains behind one protocol, one host and one credential. IMAP carries mail; CardDAV and CalDAV are two protocols and got two config keys. Graph is one protocol and got one, so the domain has nowhere to live and mail was hard-coded into it at four seams:

- `Client::media_type` answers `message/rfc822` for `Client::Msgraph(_)` as a constant, not as a session property, where the DAV arm forwards to its `DavKind`;
- `GraphClient::list_mailboxes` drives `mail_folders_list` and `mail_child_folders_list` only;
- `GraphClient::enumerate` drives `messages_delta` only;
- `MsgraphConfig` carries `user-id`, `tls`, `alpn` and `auth`, and nothing naming what is synced.

So a Graph account syncs mail and silently syncs nothing else, which reads as a misconfiguration and is not one. The spec says as much (`Sources are remote backends only`: "IMAP and Microsoft Graph for `message/rfc822`"), so the behaviour is correct and the shape is what is wrong.

Most of what a contacts arm needs already exists. io-msgraph ships the whole contacts surface: `contact_folders_list`, `contact_child_folders_list`, `contacts_delta` and `contacts_delta_from_link`, `contact_get` / `create` / `update` / `delete`, and `MsgraphContact.change_key` maps onto the `revision` the mutable-content path was built for. A Graph contact to vCard projection also exists, in cardamum's `src/msgraph/project.rs`: `to_vcard`, `to_contact`, `to_contact_delta` against a base, and a `singleValueExtendedProperties` stash carrying every line Graph has no slot for.

The backend is being taken out of the coming release, so this is what it comes back as, not a patch on what shipped.

## What

### 1. The domain is a backend, not a field

Graph gains a second protocol key, `msgraph-contacts`, beside `msgraph`. One `GraphClient` serves both, parameterised by a `GraphKind` naming which it speaks, exactly as one `DavClient` serves CardDAV and CalDAV.

This is deliberately **not** an `msgraph.kind` field. `A source's backend declares the kind it syncs` requires that the kind "SHALL be derived from the source's backend, never declared in the configuration", and a `kind` field would break that clause outright for one backend while every other keeps it. The cost is that a config key stops naming a bare protocol; `carddav` and `caldav` already set that precedent, and one client behind two keys is the same trade the DAV adapter took.

They share the `msgraph` cargo feature, one dependency and one adapter, as `dav` covers both DAV protocols.

### 2. A Graph contact is a vCard, and its identity survives the round trip

This is the risk centre, and it is not plumbing.

`text/vcard` derives its link id from the vCard `UID`, falling back to `hash:` on a card carrying none. A Graph contact has no UID: cardamum mints `UID:<graph id>` on read and drops the incoming UID on write, because the Graph id addresses the resource through the request path. Both halves of that are wrong here:

- minting from the Graph id gives the same person a Graph-local link id on one side and its real `UID` on the other, so a Graph source paired with a CardDAV one matches nothing and duplicates the whole address book on every run, forever;
- falling back to `hash:` is worse: every field edit repoints the identity, so an edit reads as a delete plus an add.

The rule: the vCard `UID` is stored in the extended-property stash on write and read back from it on read, so a card keeps the identity it arrived with. A contact with no stashed UID is one Graph itself created, and only then is a UID minted from the Graph id, stashed, and kept from then on. The link id is the vCard `UID` for both, with no Graph-specific spelling, so `One identity is one item across an account's endpoints` holds across a Graph and a DAV endpoint.

### 3. The projection moves to io-msgraph

`cardamum/src/msgraph/project.rs` is protocol projection, not product logic, and a second copy of it in neverest is how a silent-loss bug lands. It moves into io-msgraph beside the resource it projects, and cardamum takes it from there.

### 4. What mail-only means, spelled out

`sends_natively()` matches any `Msgraph` source and would offer a contacts source as the account's send channel. It, `carries_mail()` and the `smtp` refusal become kind-aware, so `A send channel belongs to at most one source` holds when an account carries two Graph sources.

`Kind::Vcard` and `Kind::Ical` are gated on the `dav` cargo feature. The gate moves onto the kind, so an `msgraph`-only build can sync cards.

### 5. Calendar is not in this change

io-msgraph has no calendar code at all: no events resource, no calendars listing, no delta. That is a from-scratch RFC-coverage milestone in the library, and Graph events to iCalendar is a harder projection than contacts by an order of magnitude (recurrence, exceptions, per-instance overrides, time zone identifiers that are not IANA names). Landing it here would hold contacts hostage to it.

`GraphKind` is written open, so the third arm is an addition rather than a reshaping, and the spec says outright that a Graph account has no calendar rather than leaving a reader to infer it.

## Not in scope

**No calendar** (§5). **No OAuth flow**: Graph auth stays a bearer token from an external broker, and a contacts source needs `Contacts.ReadWrite` in it, which ortie's documented Graph scope set (`User.Read`, `Mail.ReadWrite`, `Mail.Send`, `offline_access`) does not carry. That is an ortie doc fix, named here so the first run does not fail with a bare 403. **No contact photos**: `$value` on a contact photo is a second resource with its own ETag, and a vCard `PHOTO` round trip is its own change. **No Graph folder hierarchy beyond two levels**, matching what mail already replicates.

## Risks

| Risk | Mitigation |
| --- | --- |
| The stashed UID is the identity, and a stash that fails to write silently mints a new one | A write whose stash Graph did not accept is refused, never completed with a minted UID |
| Extended properties are dropped by `$select` unless expanded | The contacts delta carries the `$expand` clause the projection needs, asserted by a test |
| A Graph contact and a CardDAV card diverge on fields Graph has no slot for | The stash remainder already carries them; a round-trip test holds it |
| Graph contacts delta pages are per-folder and the default Contacts folder has a sentinel id | Same folder-id resolution mail uses, with the sentinel mapped to the omitted segment |
| Two Graph sources in one account double the token's scope requirements | The wizard names the scopes it needs per domain before writing the config |

## Phasing

1. Move the projection into io-msgraph, with the UID stash rule and its round-trip tests. No neverest change.
2. `GraphKind` on `GraphClient`, `media_type` forwarding, kind-aware `sends_natively` / `carries_mail`, `Kind::Vcard` off the `dav` gate. Mail behaviour unchanged.
3. `msgraph-contacts` config, sugar, wizard entry and scope guidance.
4. End to end against a real tenant, as `tests/carddav.rs` runs against Radicale.
5. Fold the delta, log, and put `msgraph` back in the default features (`msgraph-waits-for-its-domains` took it out).
