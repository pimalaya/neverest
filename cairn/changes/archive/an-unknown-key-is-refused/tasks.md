---
cairn: tasks
change: an-unknown-key-is-refused
---

# Tasks

- [x] Tests first, failing against the old code: an unknown key at the root, in a source and a target table, in a backend, an `smtp`, a `tls.rustls`, a `collection` and a `store` table; a source naming two backends or none; each removed key naming itself and MIGRATION.md.
- [x] `Config` denies unknown fields.
- [x] `SourceConfig` reads through `SourceTable` and `TryFrom`.
- [x] `folder`, `envelope`, `smtp.login`, `smtp.password` refused by name; every removed-key message links MIGRATION.md; `collection.namespace` found on the sugar.
- [x] Every key config.sample.toml documents, commented or not, still loads (new test).
- [x] CHANGELOG, MIGRATION.md.
- [x] Fold into cairn/spec/sync.md, log, archive.
