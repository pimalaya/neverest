---
cairn: tasks
change: mailbox-progress
---

# Tasks

- [x] `driver`: `MailboxProgress { spinner, label }` + `tick(done, total)` (→ `%`).
- [x] `offline/remote.rs`: `EmailRemote.on_body` (`&(dyn Fn() + Sync)`) +
      `with_progress`; call it per streamed `Full` body in the serial and pooled
      fetch paths.
- [x] Thread `MailboxProgress` from both per-mailbox loops through
      `sync_mailbox`/`sync_mailbox_single` → `propagate` → `hydrate_all` /
      `hydrate_copies` / `relay_copies`, each over a shared `AtomicUsize`.
- [x] Build/test/fmt/clippy; live multi-message hydration smoke (tick from the
      concurrent pool, no deadlock).
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
