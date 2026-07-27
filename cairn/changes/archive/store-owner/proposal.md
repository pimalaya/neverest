---
cairn: change
id: store-owner
status: landed
created: 2026-08-07
---

# Neverest as the sole pimdir store owner

## Why

The multi-process gateway architecture (decided 2026-08-07) makes neverest the
**only** process that writes a pimdir store: it syncs, applies the actions
frontends queued, and sends queued mail. Frontends (a separate connector binary)
are readers and queue producers over io-pimdir's `PimdirProducer`. io-pimdir v2
just landed with the owner surface this needs: schema migration on open, the
action-queue drain (`queued_collections` / `drain_collection` / parked rows),
and collection generations bumped atomically by `write_rekeyed`.

## What (design)

1. **io-pimdir v2 adoption**: refresh the path-patched deps (io-replica dropped
   its `client` feature); stores migrate to schema v2 on open, `user_version > 2`
   is refused.
2. **Queue drain**: at the start of every sync run, for each store,
   `queued_collections()` then `drain_collection()` each, logging applied/parked
   counts (info when nonzero) and surfacing drained counts and parked actions in
   the sync report. The subsequent sync of those collections pushes the resulting
   dirty state. Local-only collections (the Outbox) are drained but never
   remote-synced.
3. **Run-level flock**: the existing per-store `sync.lock` moves to the actual
   store directory (honouring `store.root`) and waits up to 60 s for the holder
   instead of failing immediately, so cron and connector-triggered runs
   serialize.
4. **Scoped runs**: `sync -m/--include-mailbox` already scopes a run; kept as
   the connector contract, no new surface.
5. **Handle-space rebuild**: for IMAP sides, the driver compares the stored
   checkpoint's UIDVALIDITY before and after the pull; on a change it drives
   io-replica's `ReplicaRekey` and routes its rebuild write through
   `write_rekeyed`, so `collections.generation` bumps atomically with the
   rebuild. Graph sides never bump: Graph message ids survive delta resets.
6. **Full hydration mode**: `store.hydration = "full"` makes a two-source
   retain sync hydrate every placement to `Full` (bodies mirrored); the default
   stays per-mode (one-source always full, two-source hydrates only bodies
   about to cross).
7. **Graph backend**: `msgraph` sides open for real, protocol-direct over
   io-msgraph (delta enumeration with the deltaLink as opaque checkpoint,
   HTTP 410 restarting a full round, cached delta rows at the `Meta` tier,
   `/$value` at `Full`), with the three-flow `GraphAuth` (device-code with
   refresh token persisted at `<store>/tokens.json` mode 600, client
   credentials by secret, client credentials by RFC 7523 certificate
   assertion) plus a plain bearer token. Push scope: flags via
   `message_update`, delete via `message_delete`; append/move/update are
   rejected (pull-only), documented.
8. **Outbox send channel**: the `Outbox` collection is local-only. After the
   drain, placements staged as creations in it are sent through the account's
   send channel — SMTP submission (new optional per-account `smtp` table) or
   Graph `sendMail` (auto-saves to Sent) — and dropped from the store on
   success; a per-message failure leaves it queued and never kills the run.

## Scope / non-goals

- No daemon, no watch: the connector spawns scoped `sync` runs.
- The Graph backend does not push appends/moves; the two-source mirror with a
  Graph side is pull-mostly and says so.
- Live Graph/SMTP provider validation happens later in another repo; the tests
  here cover everything scriptable locally.
