---
cairn: log
change: store-owner
landed: 2026-08-07
---

# Neverest as the sole pimdir store owner

Neverest now owns its pimdir stores end to end for the multi-process gateway
architecture: it is the only process that writes a store (sync, applying the
actions frontends queued, sending queued mail); frontends read and enqueue
through io-pimdir's producer surface.

**io-pimdir v2**: the path-patched deps refreshed (io-replica dropped its
`client` feature); stores migrate to schema v2 on open, a newer store is
refused.

**Queue drain** (`driver::drain_queues`): every run drains each queued
collection before any network work — exactly-once apply-and-delete per action,
permanently bad actions parked and re-reported by every run (report `drained` /
`parked` sections, info logs when nonzero). The subsequent sync pushes the
resulting dirty state (a drained `add` lands base-less, projecting a dirty
staged creation the push derives an append from).

**Store lock** (`cli/sync.rs::acquire_store_lock`): the `sync.lock` moved to
the actual store directory (honouring `store.root`, also fixing the
initialized-check mismatch) and a second run now waits up to 60 s (500 ms
polls) before erroring clearly, so cron and connector-triggered scoped runs
serialize. Scoped runs themselves already existed
(`sync -m/--include-mailbox`, `-x`, `-A`) and are unchanged — they are the
connector's contract.

**Handle-space rebuild** (`driver::sync_side_rebuilding` around every per-side
sync): for IMAP sides the stored checkpoint's UIDVALIDITY is compared pre/post
pull; a change drives io-replica's `ReplicaRekey` (cached bodies/summaries/
pending state carried by link id) with its single write batch routed through
`PimdirStore::write_rekeyed`, bumping `collections.generation` atomically with
the rebuild. Graph sides never bump: Graph ids survive delta resets (documented
in `msgraph/client.rs`). The guard is pre/post within one run; the crash window
between checkpoint write and rekey is documented (content still converges by
link id).

**Full hydration** (`store.hydration = "full"`, `driver::hydrate_full_mailbox`):
a two-source retain sync can mirror every body (both sides raised to `Full`,
dedup making the second side's pass fetch-free); forces retain, warns with an
explicit relay. Defaults unchanged.

**Graph backend** (`src/msgraph/{auth,client}.rs`, ported from the pimgate
prototype): `msgraph` sides now open for real — `GraphClient` under the shared
`Client` enum (folders two levels deep, delta enumeration with the deltaLink as
opaque checkpoint and 410 recovery, cached delta rows at `Meta`, raw MIME at
`Full`, flag pushes via `message_update`, deletes via `message_delete`;
append/move/mailbox mutations rejected, pull-only) and `msgraph::auth` (bearer,
device-code with `tokens.json` mode 600, client credentials by secret or
RFC 7523 certificate assertion). Deviation from the reference: no sibling
`GraphRemote` — the engine keeps already-resolved link ids on `Full` upgrades
(io-replica guarantees it), so Graph rides the existing `EmailRemote` seam
through the `Client` dispatch, which required making the seam's checkpoint
opaque bytes and its handles strings (the IMAP cursor helpers stayed in
`offline/remote.rs`). Graph pools clamp to one connection.

**Outbox** (`offline/outbox.rs`): the reserved local-only `Outbox` collection
is filtered out of every remote mailbox list (even an explicit include),
ensured at run start, and flushed right after the drain through the account's
send channel — the new per-account `smtp` table (io-smtp submission,
LOGIN-or-anonymous, quit after the drain) or the Graph sendMail action.
Envelope from the outbox meta (`{v, from, rcpts, subject}`), success drops the
placement (blob GC'd), per-message failure stays queued and reported
(`outbox` report section), channel failure warns and never kills the run.

Verified by unit/integration tests (32 green): drain via a real
`PimdirProducer` (applied add + no-op remove + parked unknown-seq), rekey over
a scripted remote (carried by link id, generation 1→2 exactly once), the lock
wait/timeout/release, the outbox flush against a scripted local SMTP sink
(envelope + bytes captured, sent placement dropped, failure stays queued),
Graph pure functions (flags, envelope fold, flags patch shape, 410
classification, checkpoint round trip, folder map), config parsing (auth
flows, smtp, hydration). fmt clean; the only clippy warning is the
pre-existing one in `wizard/autoconfig.rs`.

Spec updated: `sync` (ADDED: sole owner + drain, bounded store lock, IMAP
rebuild + generation bump, full-mirror hydration, Graph first-class side,
local-only Outbox + send channel, opaque seam checkpoint).
