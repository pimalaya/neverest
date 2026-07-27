---
cairn: change
id: mailbox-progress
status: landed
created: 2026-08-01
---

# Per-mailbox progress (a second counter)

## Why

The sync spinner shows a mailbox counter (`[2/7] Syncing INBOX`) but no feedback
about the work *inside* a mailbox. The slow inner phase is body hydration — the
`Full`-tier fetch pool downloading N messages — and the count is known, so a live
percentage on the same line (`[2/7] Syncing INBOX 66%`) tells the user what's
happening. (Enumeration is now cheap via QRESYNC, so bodies are the phase worth
surfacing.)

## What

- A `MailboxProgress { spinner, label }` bundle threaded from each per-mailbox loop
  (one/two-source) into the propagation phases; `tick(done, total)` sets the
  spinner to `<label> <percent>%` via `set_message`.
- `EmailRemote` gains an optional `on_body` callback (`&(dyn Fn() + Sync)` — the
  fetch pool calls it from several worker threads), invoked once per streamed
  `Full` body in both the serial and pooled fetch paths. `with_progress`
  constructor sets it; `new` leaves it `None`.
- The driver wires it in `hydrate_all` (one-source retain), `hydrate_copies`
  (two-source retain, one counter across both sides) and `relay_copies` (per
  relayed message, verb `relayed`), each over a shared `AtomicUsize`.

Verified: a multi-message 1-source sync hydrates every body with the tick firing
from the concurrent fetch pool (no deadlock; `set_message` from several threads is
serialised by the spinner's mutex).

## Scope / non-goals

- Progress is only surfaced for the body-fetch phase (the slow one). Enumerate and
  the `Meta` upgrade are fast and stay silent.
- No render change when stderr is not a TTY (the spinner is inert there anyway).
