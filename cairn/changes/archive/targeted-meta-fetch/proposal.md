---
cairn: change
id: targeted-meta-fetch
status: landed
created: 2026-08-01
---

# Targeted Meta / size fetches (kill the silent whole-mailbox scans)

## Why

Syncing a large mailbox (e.g. a Fastmail INBOX) is slow in the silent phases
*before* and *around* the download `%`, even when little changed. The `%`
counter only covers `Full` body streaming (pooled, fast); the lag is two
whole-mailbox `FETCH 1:* (… ENVELOPE …)` scans that bracket it and report
nothing:

1. **Link-id resolution (`Meta` tier).** `fetch_meta` lists the *entire*
   mailbox's envelopes to resolve the link id / summary of the handles it was
   handed, discarding the rest. If five messages changed, it still envelope-scans
   all fifty thousand.
2. **Largest-first scheduling.** `fetch_full` → `sizes()` lists the *entire*
   mailbox's envelopes *again*, only to read `RFC822.SIZE` — fetching the full
   ENVELOPE it never uses.

`enumerate` is already lean and targeted (`UID FLAGS` only, QRESYNC delta); the
inconsistency is that `Meta` and `sizes` never got the same treatment, so every
changed mailbox pays two full ENVELOPE sweeps regardless of the change size.

## What

Target both fetches to the handles actually being processed, mirroring
`enumerate`:

- `fetch_meta` issues a `UID FETCH <handle-set> (UID FLAGS ENVELOPE RFC822.SIZE)`
  instead of `1:*`. Incremental syncs drop from O(mailbox) to O(changed).
- `sizes()` issues a `UID FETCH <hydration-set> (UID RFC822.SIZE)` — targeted
  *and* size-only (no ENVELOPE). Even a first sync (every message new) is far
  lighter than a full ENVELOPE sweep, so the redundant second scan is gone.

Two new backend methods (`fetch_envelopes(uids)`, `fetch_sizes(uids)`) threaded
through `Client` → `EmailRemote`. A first-ever sync stays inherently heavy (each
new message's `Message-ID` is fetched once), but the *double* scan disappears and
every incremental sync gets dramatically faster. No behaviour change beyond which
UIDs are fetched; the resolved link ids, summaries and ordering are identical.
