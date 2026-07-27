---
cairn: change
id: relay-mode
status: landed
created: 2026-08-01
---

# Relay mode (two-source pass-through, no retention)

## Why

A two-source mirror (server A ↔ server B) retains every body in the hub today —
pure waste when both servers already hold it. Relay streams the body A→B directly
through a bounded buffer, the hub keeping only the spine. Bonus: download and
upload overlap, so a single transfer's wall-clock roughly halves.

## What (design)

- `StoreConfig.retention` = `Retain | Relay`, defaulting by mode: one side →
  `Retain` (the app reads bodies); two sides → `Relay` **iff both are streamable
  (IMAP)**, else `Retain`. Overridable (a browsable 2-source mirror forces
  `Retain`).
- **Driver-level relay** (the structural part): a cross-copy target — item on A
  missing on B — is streamed A→B instead of hydrated into the hub. The receiving
  side's next enumerate picks up the relayed message (its new UID) and the hub
  records B's binding, spine only, body never stored. This replaces
  `hydrate_copies` with `relay_copies` in the two-source path under `Relay`.
- **The bounded pipe**: A's `fetch_body_stream` feeds B's `append_stream` through
  a bounded reader/writer pipe on a worker thread, so bytes flow A→B without the
  whole body in memory. The exact `APPEND {N}` length comes from A's fetch literal
  (`RFC822.SIZE` / the `{N}` io-imap surfaces), so no reliance on a pre-buffer.

## Dependencies (why this is bigger than A–C)

- **Couples both connections.** Neverest drives one side at a time; relay needs A's
  and B's connections open together for one copy — a driver restructure, not a new
  `EmailRemote` method.
- **io-imap surface.** The source length must be available *before* the target
  APPEND; confirm io-imap exposes the fetch literal length up front for the relay
  reader (retain gets it from the buffered blob today).
- **io-replica.** A cross-copy that never stores the body means the projection must
  not require a hub object for a to-be-relayed item; either relay fully outside the
  engine (driver streams, engine learns of B's copy on the next enumerate) or add a
  body-less `Created{origin}` projection. Prefer the enumerate-driven path to avoid
  an engine change.
- **Costs:** loses dedup / cheap-retry / resumability (a failed copy re-fetches
  from A). Relay is opt-in, mirror-only; retain stays the default everywhere a
  local reader exists.

## Verification (landed)

Verified with a **two-Stalwart** harness (`tests/stalwart2.sh` provisions servers
A :143 and B :144): `tests/relay.rs` seeds a message on A, syncs A ↔ B under
`store.retention = "relay"`, confirms the body reached B, and asserts the relay
account's store kept **zero** object blobs (pure spine). The bounded pipe is also
unit-tested in isolation (`offline/pipe.rs`: a >buffer body streams across threads
intact; reader sees EOF on writer close). The retain path is unregressed (the
one-server `tests/stalwart.rs` still passes).

Scoped to IMAP↔IMAP: relay is enabled only when **both sides are IMAP** (default
on for that pairing, opt-out via `store.retention = "retain"`); any other pairing
retains. m2dir relay is intentionally not supported (m2dir as a neverest side is
being retired).

## Scope / non-goals

- IMAP↔IMAP first; JMAP/Graph relay waits on io-http streamed bodies, Gmail's
  base64-in-JSON is the worst fit (documented in LOCAL_STORE_PLAN §5.1).
- No change to the one-source (retain) path.
