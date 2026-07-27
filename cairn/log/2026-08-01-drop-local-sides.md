---
cairn: log
change: drop-local-sides
landed: 2026-08-01
---

# Drop local backends (m2dir) as neverest sides

neverest now manages **remotes only** — the pimdir store is the local replica, so
a local file backend as a *side* was redundant with it. Removed the `m2dir`
Cargo feature, the `io-m2dir` dependency (runtime + dev) and its patch, the whole
`src/m2dir/` module, `SideConfig::M2dir`/`M2dirConfig`, the `Client::M2dir` arm and
every dispatch/open/init branch, the `main.rs` module decl, and the m2dir `#[cfg]`
gates in `email/flag.rs`. (There was never a maildir side.) IMAP is the only
opening backend today; JMAP/Gmail/Graph still parse.

Wizard: `configure` now builds a **one-side (local-sync) IMAP/JMAP** account (the
discovered remote reconciled against the retained store) instead of m2dir-local +
remote; `edit` keeps an account's second side only if it already had one.

Tests: dropped the m2dir-seeded `stalwart.rs` and its seed helper; the relay
integration test seeds server A and verifies server B via `curl` IMAP (no local
backend), and `stalwart2.sh` provisions the two servers. Build/test/fmt green (13
unit tests), relay integration green on two live Stalwart servers, stale m2dir/
maildir doc mentions cleaned.

Spec updated: `sync` (ADDED: sides are remote backends only; MODIFIED: the pimdir
store is the sole local copy — an on-disk store is brought in via io-pimdir
conversion, not synced as a side).
