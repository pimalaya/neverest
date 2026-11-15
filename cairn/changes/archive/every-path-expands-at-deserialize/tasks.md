---
cairn: tasks
change: every-path-expands-at-deserialize
---

- [x] `TlsConfig::cert` expands through `shell_expanded_path_opt`
- [x] An absent `cert` stays `None`
- [x] Round-trip test over `store.root`, `tls.cert` and the absent case
- [x] config.sample.toml says the path is shell-expanded
- [x] CHANGELOG names the user-visible fix
