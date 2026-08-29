---
cairn: tasks
change: data-commands-describe-what-they-print
---

- [x] `SyncReport` renamed `SyncOutput`, with its nested types deriving `JsonSchema`
- [x] `check` reports a `CheckOutput` naming the mode and each endpoint it opened
- [x] `init` reports an `InitOutput` naming the store it created and the endpoints
- [x] The conflict outputs derive `JsonSchema`
- [x] `src/json_schema.rs` registers one entry per data command
- [x] The `json-schema` command is wired, aliased `json-schemas`
- [x] A test refuses a registry key naming no command
- [x] Docs: crate header, README feature line, CHANGELOG
