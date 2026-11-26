---
cairn: tasks
change: the-contract-audit-rulings
---

# Tasks

- [x] A1: account `tls` table and field-by-field inheritance (src/config.rs, src/config/tls.rs, src/config/source.rs, src/config/smtp.rs); check rows fill `tls`.
- [x] A2.1: conflict id, collection and `remoteId` in the sync report (src/sync/report.rs, src/sync/driver/itemize.rs) and `conflict list` (src/conflict/report.rs).
- [x] A2.2: conflict bodies as `{contents, encoding}` (src/conflict/report.rs).
- [x] A2.3: one `errors` list, `kind` tags, kit queue rows for parked rows (src/sync/report.rs and the driver).
- [x] A2.4: progress lines on a pipe for a full sync alone (src/main.rs, src/cli/main.rs, src/cli/sync.rs).
- [x] A2.5: `account check` on the kit's shape (src/cli/check.rs).
- [x] NEV-X1, X3, X4, X10, X11, X12, X13.
- [ ] NEV-X2: needs a cancellation hook in pimalaya-stream. Moved 2026-11-26 to its own change, cairn/changes/a-signal-cancels-a-hung-read (pimalaya-stream 0.4.1, which neverest locks, still has none).
- [x] Tests failing on HEAD first; spec folded; log; CHANGELOG.
- [x] Owner: replace the `[patch.crates-io]` path to ../cli by the kit's release pin before commit. Done: Cargo.toml holds no `[patch.crates-io]` (checked 2026-11-26 at ef809a9), pimalaya-cli comes from crates.io.
