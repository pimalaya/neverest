---
cairn: change
id: single-source-mode
status: landed
created: 2026-08-01
---

# Single-source mode (local sync) + store config

## Why

Neverest could only sync **two** sides through the implicit pimdir hub. But the
store is the local replica an app (a Himalaya pimdir backend) reads and edits, so
the natural shape for the app/device case is **one remote synced against that
retained store** — the store *is* the local side, not a second backend. There was
also no way to configure the store (it was fully implicit).

Side-count selects the mode (agreed design): **one** configured side = a *local
sync* (that remote ↔ the retained store the app reads/edits), **two** = the
existing remote-to-remote sync. This makes the store the sole local copy and gives
a device its offline, editable mailbox.

## What

- `AccountConfig.left`/`right` become `Option<SideConfig>`; a new `store`
  (`StoreConfig { root }`) customises the otherwise-implicit store at account root
  (never as a side). `init`/`check` iterate the configured sides; ≥1 is required.
- `driver::run` dispatches on side count: `run_dual` (unchanged two-source path)
  or `run_single`. `run_single` opens the store as the one side's source, syncs
  that remote against the hub, and — because the store is the app's offline copy —
  **hydrates every item to Full** (`hydrate_all`), unlike the two-source path which
  only hydrates bodies about to cross.
- The local sync reports what it pushes: it pulls first (keeping any app-staged
  edit dirty), itemizes the pending local→remote work, then pushes and settles —
  so a flag change made in the app is reported, not swallowed as "already in sync".

Verified end-to-end: a one-side (m2dir) account syncs into the store; Himalaya
lists envelopes from meta and reads hydrated bodies; a flag edited in Himalaya
(auto-sourced) propagates to the remote on the next sync, and the report shows the
`add [\seen] … on left` push.

## Scope / non-goals

- Retention for the two-source pass-through (relay) is a separate change.
- Local-side mailbox creation/deletion propagation is not added here (message-level
  sync only, as before).
- m2dir/Maildir remain usable as sides for now; demoting them to import-only
  (io-pimdir conversion scripts) is separate.
