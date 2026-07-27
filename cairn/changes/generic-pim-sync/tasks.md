---
cairn: tasks
change: generic-pim-sync
---

# Tasks

## Phase 0 — decide

- [x] **Account shape**: unchanged (`left` / `right`, one backend per side, no
      kind in the config). A store may hold several kinds; an incompatible pair
      fails at **runtime** when the sides report their media types, not at
      config load. *(decided 2026-08-07)*
- [x] `collection` / `item` naming confirmed, with `#[serde(alias)]` kept on the
      old `mailbox` / `message` spellings for one release.
- [x] `carddav` / `caldav` stay out of `default` features until they pass a live
      server run; added to `default` in Phase 6 if green.

## Phase 1 — de-mail the vocabulary

Scope call taken while landing: the **adapters keep their protocol nouns**
(`src/imap/`, `src/msgraph/` still say mailbox and message internally, and
convert at the seam). Only the layer above `client.rs` is de-mailed. This keeps
the diff honest — an IMAP mailbox *is* a mailbox — and much smaller.

- [x] `src/email/` → `src/item/`: `Mailbox` → `Collection`, `Envelope` →
      `ItemSummary` (`mailbox.rs` → `collection.rs`, `envelope.rs` →
      `summary.rs`). `ItemSummary` and `Address` are still mail-shaped and
      carry a note saying so; they move under the mail kind in phase 2.
- [x] `client.rs`: kind-neutral verbs (`list_collections`,
      `create_collection`, `delete_collection`, `fetch_summaries`,
      `get_item_stream`, `add_item_stream`, `delete_item`, `move_items`),
      `collection` parameters, `message_id` → `link_hint`.
- [x] `offline/remote.rs`: `EmailRemote` → `PimRemote`; `CachedFetchRemote`
      follows. Header states plainly that its internals are still mail-only.
- [x] `sync/hunk.rs`, `sync/report.rs`: `MailboxHunk` / `EmailHunk` →
      `CollectionHunk` / `ItemHunk`; report sections `mailbox` / `email` →
      `collection` / `item`; display strings de-mailed.
- [x] `config.rs`: `mailbox` → `collection`, `message` → `item`,
      `MailboxFilter` → `CollectionFilter`, `#[serde(alias)]` on both old
      spellings; sync-config and permission structs renamed.
- [x] `cli/sync.rs`: `--include-collection` / `--exclude-collection` /
      `--all-collections`, old long names kept as clap aliases, `-m`/`-x`/`-A`
      unchanged.
- [x] `config.sample.toml`, CHANGELOG entry (config + CLI aliases, `--json`
      break). ARCHITECTURE.md and docs/ have since been retired into the
      main.rs header and cairn/ (header-001, cairn-001).
- [x] Test: `the_pre_generic_pim_sync_spellings_still_load` — an old-spelling
      configuration loads to the same permissions and filter as a new one.
      Old CLI flags verified to still parse against the built binary.
- [x] Suite green (46 tests), fmt + clippy clean, no behaviour change.
      *(One pre-existing clippy `incompatible_msrv` warning on
      `File::try_lock` in `cli/sync.rs` is untouched and unrelated.)*
- [x] `Cargo.toml` description + keywords and README (done in phase 6, once
      CardDAV existed: advertising contacts before the backend would have been
      false).

Known staleness left alone (pre-existing, not this phase's business):
`config.sample.toml` still documents `m2dir` sides, which `cairn/spec/sync.md`
says are no longer sync sides.

## Phase 2 — kind seam

Design deviation from the proposal: `Kind` is an **enum**, not a trait. The set
of kinds is closed and small, there is one dispatch point, and it mirrors how
`Client` already dispatches over its backends; a trait would buy dynamic
dispatch nobody needs.

- [x] `src/kind/`: `Kind` enum with `from_media_type` / `media_type` /
      `parse_body` (the `Full` derivation every kind has) / `parse_summary`
      (the cheap `Meta` derivation, `None` for a kind without one — which is
      how the DAV kinds will declare "resolve at `Full` only").
- [x] `kind::mail`: the `mid:` / `alt:` link id, `envelope_date` and the
      `MetaSummary v:1` schema moved out of `offline/remote.rs` verbatim, with
      all four of their tests. The module header now records *why* the two
      derivations must agree byte-for-byte (the `Z` vs `+00:00` bug).
- [x] `PimRemote` carries a `kind`, resolved once from `Client::media_type()`;
      `parse_summary` at the `Meta` tier, `parse_body` at `Full`. `Kind` is
      threaded into the two free body-parsing helpers (`fetch_one_full`,
      `hydrate_batch`) and resolved once per hydrate phase in the driver.
- [x] `driver`: `check_kinds` / `pair_kind` refuse an unknown media type or a
      cross-kind pair right after the sides open and before any store write
      (Phase 0 decision). The one-side path checks the kind is known.
- [x] Test: `a_side_pair_must_agree_on_its_kind` over the pure `pair_kind`,
      plus the four relocated mail-kind tests (47 total, green; clippy clean
      but for the pre-existing MSRV warning).
- [ ] Per-kind display nouns (mailbox / address book / calendar, message /
      contact / event) — **moved to phase 6**. Doing it properly means the
      `SyncReport` carries the kind, which is report-shape work; threading a
      noun through three progress helpers for cosmetics was not worth it now.
      (An unused `Kind::collection_noun` was written and then removed rather
      than left dead behind an `allow`.)
      **Still open after phase 6**: the report would have to carry the kind, so
      it is report-shape work rather than a rename, and a contacts run reading
      "mailbox" is cosmetic. Deferred to its own change.

## Phase 3 — mutable content

- [x] `Client::update_item_stream`; `EnumEntry.revision`; `get_item_stream`
      returns the revision; `fetch_bodies`' `done` callback receives it;
      `add_item_stream` returns a `WrittenItem { id, revision }`;
      `delete_item` takes `if_match`.
- [x] `PimRemote`: revision carried through enumerate and both fetch paths;
      `ReplicaChange::Update` implemented as a conditional streamed write
      (rejection reported as `Rejected`, **not** an error, so the engine
      re-merges instead of aborting the batch); `Remove` honours `if_match`.
- [x] `Client::handle_space_epoch(&checkpoint) -> Option<u64>`; the IMAP
      checkpoint codec moved into `src/imap/backend.rs` with its test. The
      driver's guard now reads an opaque epoch, so `SideCtx::imap` **is gone**
      — "does this backend have a handle space" is the backend's answer, not a
      flag the driver carries.
- [x] Config: `item.update` (defaults to `true` via `#[serde(default)]`, so a
      configuration predating it still parses — `create`/`delete` stay
      required, preserving the declare-in-full rule). Sample config documents
      it.
- [x] **`SidePermissions` → `ReplicaPushRights` properly mapped.** This closed
      a standing TODO: `writable()` collapsed every permission into one
      all-or-nothing boolean and the per-kind rights io-replica already had
      were never used, so `item.delete = false` did nothing. Now
      `flag.update → flags`, `item.update → content`, `item.create → add`,
      `item.delete → remove`.
- [x] `report`: `ItemHunk::Update` variant (emitted when a `Dirty` placement's
      body pointer moved off its base) + a conflicts section counted as
      warnings, in text and `--json`.
- [x] Tests (52 green, clippy clean): a `MutableRemote` fake with ETag
      semantics proves the update is pushed `If-Match` and confirmed; a
      both-sides edit leaves the remote untouched and reports the conflict;
      `item.update = false` keeps the edit pending and never reaches the
      remote; the permission→rights mapping; two config tests for the new gate.

### ⚠ Blocker found for phase 4: the pimdir store loses conflict state

The fake-remote test did its job and caught this before any DAV code exists.

io-replica's engine handles a both-sides edit correctly — remote untouched,
`conflicts: 1`, a `Conflicted` event — but the state **does not survive the
round trip through the pimdir store**. `io-replica/src/hub.rs` drops it in both
directions:

- `absorb` always writes `conflicted: false` / `conflict_object: None`,
  whatever the placement's status;
- `bound_placement` only ever projects `Clean`/`Dirty`/`Tombstone`/`Created`,
  with a hardcoded `conflict_revision: None`.

The fields exist on `ReplicaHubItem` *and* as pimdir columns (`conflicted`,
`conflict_object`) — they are simply not wired to `ReplicaStatus::Conflict`.

Consequence: a conflicted placement reads back `Dirty`, so the next run
re-derives the same push, re-conflicts and re-reports, forever, and a frontend
reading the store cannot tell which items need resolving. **Inert for mail**
(immutable bodies never conflict), **fatal for CardDAV/CalDAV** — hence a
phase 4 blocker.

`a_body_edited_on_both_sides_is_left_conflicted_not_overwritten` asserted that
lossy behaviour deliberately, as a canary: it fails the moment io-replica
round-trips the conflict, which is the fix landing.

**Resolved 2026-08-07.** io-replica's `hub-conflict-round-trip` change carries
the conflict and its revision on `ReplicaSourceBinding`, and io-pimdir's
`persist-binding-conflict` change persists both (`bindings.conflicted` /
`bindings.conflict_revision`). The canary fired as designed and is flipped: the
test now asserts the placement reads back `Conflict` with
`conflict_revision == Some("v9")`. Both fixes are in their working trees and
unpublished, so `Cargo.toml` patches them until they ship.

- [x] **Cross-repo fix in io-replica** before phase 4: wire `absorb` /
      `bound_placement` to `ReplicaStatus::Conflict` + `conflict_revision`;
      then flip the canary assertion and restore the `conflict_revision`
      check.

## Phase 4 — CardDAV

*Unblocked: the conflict round-trip above landed on 2026-08-07.*

- [x] `carddav` cargo feature; io-webdav dependency (published 0.1, no patch needed).
- [x] Live-server harness: io-webdav already ships `tests/radicale.sh` +
      `tests/radicale.rs` (Docker); reuse that shape here rather than writing a
      new one, alongside the existing `tests/stalwart*.sh`.
- [x] `config`: `CarddavConfig` side block (server URL, TLS, ALPN, auth) via
      the `side_config!` macro; reject an `smtp` table on a DAV side.
- [x] `src/carddav/client.rs`: connect + principal/home-set discovery;
      `list_collections`; `enumerate` via `REPORT sync-collection` (token as
      opaque checkpoint, `valid-sync-token` → full `PROPFIND`, `complete: true`);
      `fetch_bodies` via `addressbook-multiget`; `add`/`update`/`delete` with
      `If-Match` / `If-None-Match`; move as create + delete.
- [x] `kind::vcard`: `uid:` link id with a `hash:` fallback, `text/vcard v:1`
      meta.
- [x] DAV items resolve at `Full` only (no `Meta` tier); flags reported
      known-empty (`'[]'`), with a test asserting it is not `NULL`.
- [x] `client.rs`: `Client::Carddav` arms, `media_type()` → `text/vcard`,
      `handle_space_epoch` → `None`.
- [x] Tests: pure codec/parse units + a scripted live-server run beside
      `tests/stalwart*.sh` (Radicale or Baïkal).

## Phase 5 — CalDAV (deferred to its own change, 2026-08-07)

*Out of scope here: the shape this change established (kind seam, DAV adapter,
live suite) is what CalDAV builds on, so it lands as its own change rather than
holding this one open.*

- [ ] `caldav` cargo feature; `CaldavConfig`; `src/caldav/client.rs` in the
      Phase 4 shape over RFC 4791 (`calendar-multiget`).
- [ ] `kind::ical`: `uid:` link id (RECURRENCE-ID excluded),
      `text/calendar v:1` meta carrying the component.
- [ ] `Client::Caldav` arms; tests mirroring Phase 4.

## Phase 6 — wizard, docs, spec

- [x] `wizard/search.rs`: keep `Caldav` / `Carddav` discovery entries and offer
      them, gated on compiled-in backends; multi-service runs offer one account
      per kind against a shared `store.root`.
- [x] `config.sample.toml`: CardDAV account (CalDAV with its own change).
- [x] `README.md` and `CHANGELOG.md` (MIGRATION.md needs nothing: contacts are new, not migrated).
- [x] ARCHITECTURE.md and docs/ retired: the crate architecture moved into the
      main.rs header (header-001) and the behavioural contract into cairn/
      (cairn-001), dropping the stale io-email and m2dir descriptions with them.
- [x] **Cross-repo**: pimdir SPEC §13 gained the `text/vcard` convention; `text/calendar` follows with CalDAV. Was:
      `text/calendar` `v:1` conventions.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; write
      `cairn/log/YYYY-MM-DD-generic-pim-sync.md`; set `status: landed`.
