---
cairn: change
id: drop-local-sides
status: landed
created: 2026-08-01
---

# Drop local backends (m2dir) as neverest sides

## Why

Neverest is a remote↔store / remote↔remote synchroniser: the pimdir store is the
local replica, and a local file backend as a *side* is redundant with it (a body
would live both in the file store and in pimdir). Local file stores belong on the
import/export path (io-pimdir conversion), not as sync sides. neverest carried an
`m2dir` side (there was never a maildir one); this removes it so neverest manages
**remotes only** (IMAP today; JMAP/Gmail/Graph parse, pending their lean backends).

## What

- Removed the `m2dir` Cargo feature, the `io-m2dir` dependency (runtime + dev) and
  its patch, and the whole `src/m2dir/` module.
- Removed `SideConfig::M2dir` / `M2dirConfig`, the `Client::M2dir` arm and every
  dispatch/open/init branch, and the m2dir `#[cfg]` gates in `email/flag.rs`.
- Wizard: `configure` now produces a **one-side (local-sync) IMAP/JMAP** account
  (the discovered remote reconciled against the retained store) instead of
  m2dir-local + remote; `edit` keeps an account's second side only if it already
  had one.
- Tests: dropped the m2dir-seeded `stalwart.rs`; the relay integration test now
  seeds server A and verifies server B via `curl` IMAP (no local backend), and a
  two-server harness `stalwart2.sh` provisions A :143 / B :144.

## Scope / non-goals

- No functional change to the IMAP path, the one/two-source modes, or relay.
- Maildir/m2dir interchange is io-pimdir's job (conversion scripts); neverest docs
  point there rather than syncing them directly.
