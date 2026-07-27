---
cairn: log
change: relay-mode
landed: 2026-08-01
---

# Relay mode (two-source pass-through, no retention)

A two-side sync can now **relay** instead of retain: a cross-copy body is streamed
straight from its holding side to the other through a bounded in-memory pipe, the
store keeping only the spine (no object blob at rest). This is the pure
pass-through mirror — the storage waste of holding a body both servers already
have is gone, and download/upload overlap through the pipe.

`config.rs`: `StoreConfig.retention` = `Retain | Relay`. `offline/pipe.rs`: a
bounded cross-thread pipe (a `Write` end feeding a `Read` end, blocking full/empty)
so a body flows without being held whole. `driver`: `propagate` dispatches
`hydrate_copies` (retain) vs `relay_copies` (relay); `relay_copies` reads the
one-sided items from the hub (`relay_targets`, length from the `v:1` meta `size`)
and `relay_one` runs a scoped-thread fetch→append through the pipe (length-prefixed
APPEND). The relayed message is picked up by the target's next enumerate and bound
in the hub with no object.

Retention decision: relay is **IMAP-first** — the default for an IMAP↔IMAP pairing,
retain for any other (a non-IMAP side, or `store.retention = "retain"`; an explicit
`relay` on a non-IMAP pairing warns and retains). m2dir relay is not supported
(m2dir as a neverest side is being retired). Relay trades away dedup / cheap retry
/ resumability; retain stays the default wherever a local reader exists (every
local/one-side sync).

Verified with a two-Stalwart harness (`tests/stalwart2.sh` → A :143, B :144):
`tests/relay.rs` relays a message A→B and asserts the body arrived **and** the
relay store kept **zero** blobs. The bounded pipe is unit-tested in isolation
(`offline/pipe.rs`: a >buffer body streams across threads intact; EOF on close).
Retain unregressed (one-server `tests/stalwart.rs` still green). fmt clean; the
only clippy warning is pre-existing in `wizard/autoconfig.rs`.

Spec updated: `sync` (ADDED: a two-source sync may relay instead of retain).
