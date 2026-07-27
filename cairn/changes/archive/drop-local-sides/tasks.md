---
cairn: tasks
change: drop-local-sides
---

# Tasks

- [x] Remove `m2dir` feature + `io-m2dir` deps (runtime/dev) + patch; delete
      `src/m2dir/`.
- [x] Remove `SideConfig::M2dir`/`M2dirConfig`, `Client::M2dir` arms, open/init
      branches, `main.rs` module, `email/flag.rs` gates.
- [x] Wizard: one-side IMAP/JMAP account by default (`discover`), keep a second
      side only if present (`edit`).
- [x] Tests: delete `stalwart.rs` + seed helper; relay test seeds/verifies via
      `curl` IMAP; add `stalwart2.sh` (two servers).
- [x] Build/test/fmt; relay integration green; stale doc mentions cleaned.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
