---
cairn: tasks
change: graph-domains-share-one-table
---

# Tasks

- [x] The owner rules on the shape: D16 of the CLI alignment plan, 2026-11-15, "refuse it, go with the design as written"
- [x] Before any configuration code: `the_graph_tables_keep_their_source_ids` on today's code, passing, its expected `(source id, domain, item.delete)` triples fixed; the change rewrote only its documents
- [x] Before any configuration code: `the_one_graph_table_finds_what_the_three_tables_synced` (src/offline/driver.rs, module `graph_one_table`), run 1 landing mail, contacts and calendar on today's code, failing at the load of the new shape
- [x] src/config.rs: `MsgraphTable` holding the session (`user-id`, `tls`, `alpn`, `auth`, `proxy`) and the `mail`, `contacts`, `calendar` domain tables (`MsgraphDomainConfig`: `collection`, `flag`, `item`, `pool-size`), each a bool or a table (`msgraph_domain`); `AccountConfig` refuses a table naming no domain (`msgraph_sugar`), `SourceTable` takes one domain (`MsgraphTable::domain`); the sugar names the domains `msgraph`, `msgraph-contacts`, `msgraph-calendar` (`SourceBackendConfig::sugar_name`); the three `SourceBackendConfig` Graph variants and `MsgraphConfig` stay as the engine reads them, `MsgraphConfig` no longer read from TOML
- [x] src/config.rs: `removed_key!` for `msgraph-contacts`, `msgraph-calendar` (account, `SourceTable`) and `msgraph.collection`, `msgraph.item`, `msgraph.flag`, `msgraph.pool-size`; `mailbox` and `message` refused in the table and in a domain; the "declares no backend" list
- [x] `BACKENDS` and `is_built` lose the two tables; `protocol()` reads `msgraph` for every domain, so `account list` and an `account check` row name the table
- [x] Tests: nine refusals in `a_removed_key_points_at_the_migration_notes`; `a_graph_domain_is_true_or_a_table`; `a_graph_table_names_its_domains`; the fixtures writing `msgraph-contacts.`, `msgraph-calendar.` or a bare `msgraph.` (src/config.rs, src/offline/capability.rs, src/offline/driver.rs, src/cli/check.rs) rewritten, their source names kept; tests/common/mod.rs `Replica::open` writes a Graph source as its domain of the one table, tests/msgraph.rs `user_line` names `msgraph` (live, not run here)
- [x] A mutation (the contacts sugar named `msgraph-contact`) fails both design tests
- [x] config.sample.toml, README: the Graph section as one table
- [x] CHANGELOG (**BREAKING**, upgrade list), MIGRATION.md (a before and after for each refused spelling)
- [x] Fold delta.md into cairn/spec/sync.md, log, archive
- [ ] Tell the live tester: rc files writing `msgraph-contacts`, `msgraph-calendar` or a bare mail `msgraph` are refused; the jmap-sources change's J3 assumes Graph's split (the main session)
