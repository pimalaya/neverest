---
cairn: log
change: mailbox-progress
landed: 2026-08-01
---

# Per-mailbox progress (a percentage)

The sync spinner now reports progress *inside* a mailbox, not just the mailbox
counter: while hydrating bodies (the slow inner phase) it appends a percentage to
the line — `[2/7] Syncing INBOX 66%` — updated per streamed `Full` body. Fast
phases (the now-cheap QRESYNC enumerate, the `Meta` upgrade) stay silent.

`EmailRemote` gained an optional `on_body` callback (`&(dyn Fn() + Sync)`) invoked
once per streamed body in both the serial and pooled fetch paths; `with_progress`
sets it. The driver defines `MailboxProgress { spinner, label }` with a
`tick(done, total)` that sets `<label> <percent>%`, threaded from each per-mailbox
loop through `sync_mailbox`/`sync_mailbox_single` → `propagate` → `hydrate_all`,
`hydrate_copies` (one counter across both sides) and `relay_copies` (per relayed
message), each over a shared `AtomicUsize`. The tick fires from the concurrent
fetch pool; `Spinner::set_message` serialises the concurrent updates on its mutex.

(Initially built as `— fetched i/N`, changed to a plain percentage per request.)

Verified: a multi-message 1-source sync hydrated every body with the tick firing
from the pool — no deadlock; 14 unit tests green, fmt/clippy clean (one pre-existing
autoconfig warning), relay unregressed.

Spec updated: `sync` (ADDED: the spinner reports in-mailbox progress).
