---
cairn: tasks
change: sources-register-through-io-pimdir
---

# Tasks

- [ ] Read the lane's version (.lanes/night3/neverest `git diff -- src/sync/storage.rs src/sync/driver/local.rs src/sync/driver/pair.rs`) and port it by hand onto HEAD.
- [ ] The existing registration tests pass unchanged (a declared calendar carries its `calendar.component.vtodo` row from the listing on; a checkpoint already there is never overwritten).
- [ ] Spec folded, log, no CHANGELOG line (no behaviour change).
