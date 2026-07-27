---
cairn: log
change: single-source-mode
landed: 2026-08-01
---

# Single-source mode (local sync) + store config

Neverest now selects its mode by side count: **one** configured side is a *local
sync* (that remote ↔ the retained pimdir store the app reads/edits — the store is
the local side), **two** is the existing remote-to-remote sync. This is the
app/device shape and answers "how is pimdir configured" — it is the implicit local
replica, customised only at account root.

`config.rs`: `left`/`right` are now `Option<SideConfig>`; a new `StoreConfig
{ root }` under `AccountConfig.store` overrides the otherwise-implicit store dir; a
`sides()` helper + `SideName`. `init`/`check` iterate the configured sides and
require ≥1. `driver::run` dispatches `run_dual` (unchanged) or `run_single`.
`run_single` opens the store as the one side's source and, because the store is
the app's offline copy, hydrates **every** item to `Full` (`hydrate_all`) — unlike
the two-source path, which only hydrates bodies about to cross. It pulls before
pushing and itemizes the pending local→remote work, so an app-staged flag edit is
reported and pushed, not swallowed as "already in sync".

Verified end-to-end (m2dir single side): the account syncs into the store;
Himalaya lists envelopes from meta and reads hydrated bodies; a `\Seen` edited in
Himalaya (auto-sourced, no config) propagates to the remote on the next sync, and
the report shows `add [\seen] … on left`.

Spec updated: `sync` (ADDED: side count selects the mode; ADDED: a local sync
retains every body; MODIFIED: two sources over one store is the two-side case).
