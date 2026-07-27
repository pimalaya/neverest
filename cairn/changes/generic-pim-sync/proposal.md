---
cairn: change
id: generic-pim-sync
status: landed
created: 2026-08-07
---

# Neverest as a generic PIM sync

## Why

Neverest calls itself "CLI to synchronize emails", but nothing it stands on is
mail-shaped any more:

- **io-replica** is already "collections of PIM items: mailboxes of messages,
  address books of contacts, calendars of events". Its seams speak
  collection / placement / object / link id / flags / revision. It has a
  mutable-content path (`ReplicaChange::Update`, `revision`, `conflicted` /
  `conflict_object`) that neverest never exercises.
- **pimdir** is generic by construction: `collections.kind` is a media type,
  `ensure_collection(collection, kind)` is already plumbed, the queue's six
  actions "address collections and public ids, never protocol concepts", and
  SPEC §13 defines meta conventions *per kind* with `message/rfc822` as merely
  the first one written.
- **io-pim-discovery** already returns `DiscoveryService::Caldav` /
  `Carddav` from the same parallel compose fan-out the wizard drives, with
  RFC 6764 `.well-known` and SRV resolution.
- **io-webdav** already ships RFC 6578 `sync-collection` (the checkpoint
  analogue of QRESYNC) and ETags on every card/item verb (the revision).

So the mail-specificity is confined to neverest's own ~10k lines. This change
removes it and lands CardDAV and CalDAV sides, making neverest a PIM sync whose
mail support is one kind among three.

The payoff is not just "contacts too": a single pimdir store holding mail,
contacts and calendars, owned by one process, is what the gateway architecture
(see `store-owner`) needs for a frontend to project all three from one database.

## Where the mail-specificity actually lives

Five bands, in increasing order of difficulty:

1. **Vocabulary** — `src/email/{address,envelope,flag,mailbox}.rs`; the
   `MailboxHunk` / `EmailHunk` report DTOs; the config's `mailbox` /
   `message` tables and per-side permission triples. Pure naming.
2. **The client seam** — `Client`'s verbs are mail nouns
   (`list_mailboxes`, `fetch_envelopes`, `add_message_stream`,
   `move_messages`, `delete_message`). Pure naming; the *shapes*
   (`Enumeration` with opaque checkpoint bytes and string handles) are
   already backend-neutral, as `client.rs`'s own header says.
3. **Link id and meta** — `mid:` / `alt:` and the `MetaSummary` `v:1` schema
   are hard-coded in `offline/remote.rs`. Needs a seam, not a rename.
4. **The immutable-content assumption** — `revision` is `None` at every
   construction site and `ReplicaChange::Update` is unconditionally rejected
   ("mail content is immutable, so an in-place update never arises"). Contacts
   and events are *mutable-content*: an edit repoints the body, the ETag is the
   revision, and io-replica's conflict machinery goes live for the first time.
   **This is the only band that is new behaviour rather than renaming, and it is
   the risk centre of the change.**
5. **Mail-only side features** — the Outbox / SMTP send channel, relay mode,
   the QRESYNC checkpoint codec and its UIDVALIDITY rebuild guard, batched
   `UID FETCH` with largest-first ordering. These stay; they become
   *kind-gated* rather than universal.

## What (design)

### 1. The kind falls out of the backend; a mismatched pair fails at runtime

**Decided 2026-08-07.** The account schema is **unchanged**: an account is still
`left` / `right`, each side still exactly one backend, and no kind is ever
declared in the config. Adding `carddav` / `caldav` variants to
`SideBackendConfig` makes the kind fall out of the side's backend through the
`Client::media_type()` that already exists, and pimdir records it on the
collection with no further plumbing. No new config surface, no per-service
nesting, no restructuring of the side/permission/report machinery.

A store is therefore free to hold several kinds — which is what pimdir is built
for — and the config stays as small as it is today.

The cost is that the schema cannot express "these two sides are compatible", so
a mail side paired with an address-book side is **refused at runtime**, when the
sides open and report their media types, not at config load. That is a
deliberate trade: the check is one comparison in the driver against a hard error,
and it buys back the whole config-validation layer the alternative would need.
Opening the sides is already the first thing `init`, `check` and `sync` do, so
the error surfaces immediately and before any store write.

### 2. De-mail the vocabulary

- `src/email/` → `src/item/`: `Mailbox` → `Collection`, `Envelope` →
  `ItemMeta`, `Flag` and `Address` keep their names (`Address` moves under the
  mail kind module, being an RFC 5322 concept).
- `Client` verbs: `list_collections`, `create_collection`, `delete_collection`,
  `enumerate`, `fetch_meta`, `fetch_bodies`, `get_item_stream`,
  `add_item_stream`, `delete_item`, `move_items`, `store_flags`,
  **`update_item_stream`** (new, §4).
- `EmailRemote` → `PimRemote`; `MailboxHunk` / `EmailHunk` →
  `CollectionHunk` / `ItemHunk`.
- Config: `mailbox` → `collection`, `message` → `item`, both keeping a
  `#[serde(alias)]` on the old spelling for one release. `MailboxFilter` →
  `CollectionFilter`.
- Package description / keywords / README / MIGRATION follow.

`--json` report keys change (`mailbox` → `collection`). Breaking, called out in
the CHANGELOG; acceptable at `1.0.0-rc`.

### 3. A kind seam for link id and meta

A small `src/kind/` module, one implementation per media type, holding the two
things that are genuinely per-kind:

```rust
trait ItemKind {
    const MEDIA_TYPE: &'static str;
    /// The cross-collection identity, from a parsed body (or header prefix).
    fn link_id(body: &[u8]) -> ReplicaLinkId;
    /// The `v:1` summary a reader renders a list from (pimdir SPEC §13).
    fn meta(body: &[u8], size: u64) -> ReplicaMeta;
}
```

- `message/rfc822`: today's `mid:` / `alt:` fallback and `MetaSummary`, moved
  verbatim, tests and the date-formatting invariant included.
- `text/vcard`: link id is `uid:<UID>`; a card with no `UID` is malformed, so
  the fallback is `hash:<content hash>` rather than a synthesised digest.
  Meta `v:1`: `{v, uid, fn, org?, email?, tel?, rev?}`.
- `text/calendar`: link id is `uid:<UID>` (RECURRENCE-ID excluded — a
  recurrence override rides the same item). Meta `v:1`:
  `{v, uid, component, summary, dtstart?, dtend?, rrule?, status?, organizer?}`.

The two new conventions are a change to the **pimdir SPEC §13**, which lives in
`pimalaya/pimdir` — a cross-repo dependency this change must land alongside.

`PimRemote` selects the implementation from `Client::media_type()`; there is
one dispatch point, not a per-call-site match.

### 4. Mutable content, end to end

The band that is real work. A DAV item's body changes in place, so:

- **Revision.** `ReplicaFetchedItem.revision` carries the ETag;
  `ReplicaPushResult.revision` carries the ETag the server returned from the
  write. io-replica compares revisions instead of bytes for mutable backends,
  which is exactly what it was built to do.
- **Update.** `ReplicaChange::Update { handle, object, .. }` becomes a real
  push: `PUT` the stored blob to the href with `If-Match: <base revision>`. A
  `412` is `Rejected` — the engine re-merges and marks the placement
  `conflicted` with the observed remote body as `conflict_object`.
- **Add.** `PUT` to a fresh href with `If-None-Match: *`; the assigned handle is
  the href. **Delete.** `DELETE` with `If-Match`.
- **Move.** CardDAV/CalDAV servers do not reliably support cross-collection
  `MOVE`, so a refile is create-on-target + delete-on-source, and the report
  says so.
- **Permissions.** The per-side triple gains `item.update`; a side with
  `item.update = false` drops update hunks into the report like every other
  permission, making a DAV side genuinely read-only.
- **Conflict surfacing.** `conflicted` / `conflict_object` are stored today and
  shown nowhere. A report section lists conflicted items per collection. An
  interactive resolver (`neverest conflicts`) is a **non-goal** here; the
  report plus the pimdir queue's `update` action is enough for a frontend to
  resolve one.

**Sequencing trick:** all of §4 is testable against an in-memory fake remote
*before any DAV code exists*. Prove the mutable path there, then plug DAV in —
so a CardDAV failure is a protocol bug, never an engine bug.

### 5. CardDAV and CalDAV sides

`src/carddav/` and `src/caldav/` over io-webdav, in the same lean protocol-direct
shape as `src/imap/` and `src/msgraph/` (neverest owns its adapters; io-email
was retired and io-addressbook / io-calendar's `list_cards` / `list_items`
surface has no cursor, so their unified clients are not the right seam here —
io-webdav is).

- **Enumerate**: `REPORT sync-collection` with the stored token; the checkpoint
  bytes are the token verbatim (UTF-8). An invalid token (the
  `valid-sync-token` precondition) falls back to a full `PROPFIND` snapshot
  with `complete: true`, which io-replica already knows how to diff.
- **Checkpoint codec.** `encode_checkpoint` / `decode_checkpoint` /
  `checkpoint_uid_validity` are IMAP-specific and currently live in
  `offline/remote.rs` precisely so the driver's rebuild guard can read them.
  They move behind a `Client::handle_space_epoch(&checkpoint) -> Option<u64>`,
  `None` for every backend without one. The driver's rekey branch then reads as
  "the epoch moved", not "UIDVALIDITY changed".
- **Fetch**: `addressbook-multiget` / `calendar-multiget` for a batch of hrefs —
  the analogue of the batched `UID FETCH`, reusing the same batching structure.
- **DAV items go straight to `Full`.** `sync-collection` returns hrefs and ETags
  only — no UID — so a `Meta` tier would have to fetch the body anyway to learn
  the link id. Collapsing the tiers for DAV (bodies are kilobytes) removes the
  entire class of bug that `stable-alt-link-id` fixed for mail: there is no
  second path that could compute the link id differently.
- **Flags.** DAV has no flags. An enumerate MUST report **known-empty** (`'[]'`),
  never *unknown* (`NULL`) — the distinction is normative in pimdir SPEC §11 and
  getting it wrong makes the engine re-probe every item forever.
- **Feature gates**: `carddav`, `caldav`, mirroring `imap` / `msgraph`, added to
  `default` once they pass the live-server tests.

### 6. Kind-gating the mail-only features

- **Outbox / send channel**: exists only for a `message/rfc822` store. An
  `smtp` table on a DAV side is a config error, not a silently ignored key.
- **Relay mode**: already spec'd as IMAP↔IMAP only; the wording becomes
  explicit that it is a mail feature.
- **Largest-first `Full` ordering**: driven by the meta `size`, which the DAV
  kinds do not carry; degrades to handle order (already the empty-`sizes`
  branch).

### 7. Wizard

`wizard/search.rs` filters the discovery fan-out to IMAP/SMTP entries; it keeps
`Caldav` / `Carddav` entries too and offers them as selectable sides, subject to
the existing "only compiled-in backends are proposed" rule. Everything else —
the single email-address prompt, the derived account name, the deadline, the
connection test before writing — is unchanged. A wizard run that finds mail *and*
DAV services offers to write one account per kind against a shared store.

## Scope / non-goals

- No new sync semantics: the engine, the store and the three-way merge are
  untouched. This change teaches neverest to *speak* what they already model.
- No interactive conflict resolution command (report-only, see §4).
- No VJOURNAL/VTODO-specific handling beyond carrying the component in meta.
- No Google Contacts / Google Calendar REST backends (io-people exists; a later
  change, same shape as `msgraph`).
- No multi-kind accounts (see §1's open decision).
- io-addressbook / io-calendar stay uninvolved: the adapters go direct to
  io-webdav, as the IMAP and Graph adapters go direct to their protocol crates.

## Risks

| Risk | Mitigation |
| --- | --- |
| The mutable-content path is untrodden in io-replica | Prove it against a fake remote before any DAV code (§4) |
| `NULL` vs `'[]'` flags silently re-probing forever | Explicit requirement + a test asserting known-empty |
| ETag semantics vary across servers (weak ETags, quoting) | Normalise at the adapter edge; test against a real server in `tests/` alongside the existing stalwart scripts |
| A DAV server without `sync-collection` | Full `PROPFIND` fallback, already the `complete: true` path |
| `--json` shape break for existing consumers | CHANGELOG + MIGRATION; pre-1.0 |
| ARCHITECTURE.md is already stale (still describes io-email `EmailClientStd` and m2dir sides that sync.md says were dropped) | Retired it, with docs/, into the main.rs header and cairn/ (header-001, cairn-001) rather than refreshing drift |

## Phasing

Each phase leaves the tree green, `--json` stable within the phase, and is
reviewable alone.

0. **Decide** — single-kind vs multi-kind accounts; the `collection`/`item`
   naming; whether DAV ships in `default` features immediately.
1. **De-mail the vocabulary** — mechanical rename, no behaviour change.
2. **Kind seam** — extract link id + meta; mail becomes one implementation.
3. **Mutable content** — revision / `Update` / `item.update` permission /
   conflict reporting, proven against a fake remote.
4. **CardDAV** — the first non-mail sync.
5. **CalDAV** — the same shape over RFC 4791.
6. **Wizard, docs, spec** — DAV wizard entries, sample config, README /
   MIGRATION refresh, pimdir SPEC §13 conventions (cross-repo),
   fold this delta into `cairn/spec/sync.md`.
