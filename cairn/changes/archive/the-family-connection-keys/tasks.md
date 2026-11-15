---
cairn: tasks
change: the-family-connection-keys
---

# Tasks

- [x] pimalaya-config 0.5.1 (`cargo update -p pimalaya-config`)
- [x] `opt_shell_expanded_path` on `store.root` and `tls.cert`, the TODO helper deleted; `$VAR` test failing on the tilde-only helper
- [x] `pimalaya_config::duration::option` on `store.purge-after` and `store.index-timeout`; integer seconds test
- [x] DAV `home`, optional `server`, `auth = "none"` (src/config.rs, src/account.rs `DavEndpoint`, src/dav/client.rs)
- [x] IMAP `sasl-ir`, `id.auto`, `id.fields` (src/config.rs, src/imap/client.rs `auto_id`)
- [x] `proxy.*` at the account, every backend and `smtp`, threaded to every connection (IMAP, SMTP, DAV, Graph, People, Calendar, Gmail)
- [x] config.sample.toml, CHANGELOG
- [x] Fold into cairn/spec/sync.md, log, archive
