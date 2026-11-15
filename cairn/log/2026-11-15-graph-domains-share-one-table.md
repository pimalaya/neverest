---
cairn: log
change: graph-domains-share-one-table
date: 2026-11-15
---

# Graph's domains share one table

Line N7 of the CLI alignment round of 2026-11-15 (audit finding F37), ruled by the owner as D16: "refuse it, go with the design as written".

## What landed

- src/config.rs: `MsgraphTable`, the one `msgraph` table: the session (`user-id`, `tls`, `alpn`, `auth`, `proxy`) and the domain tables `mail`, `contacts`, `calendar` (`MsgraphDomainConfig`: `collection`, `flag`, `item`, `pool-size`), each `true`, `false` (absent) or a table. Under the account it is sugar for one source a domain, named `msgraph`, `msgraph-contacts` and `msgraph-calendar` (`SourceBackendConfig::sugar_name`), the ids the three tables gave; under `sources.<name>` or `targets.<name>` it names one domain and the source keeps `<name>`. The engine is untouched: the parse yields the same three `SourceBackendConfig` Graph variants, `MsgraphConfig` built from the table and the domain.
- Refused, as the file is parsed: the `msgraph-contacts` and `msgraph-calendar` tables (account, source, target) and `msgraph.collection`, `msgraph.item`, `msgraph.flag`, `msgraph.pool-size`, in the family sentence; an `msgraph` table naming no domain ("write `msgraph.mail = true` to sync mail as before"); a named source's table naming two domains or more. `mailbox` and `message` are refused in the table and in a domain as in every backend table.
- `BACKENDS` and `is_built` lose `msgraph-contacts` and `msgraph-calendar`; `protocol()` reads `msgraph` for every domain, so `account list` and an `account check` row's `backend` name the table.
- Tests, the design's two first: `the_graph_tables_keep_their_source_ids` (src/config.rs) passed on the three tables with its expected ids, kinds and rights, then had only its documents rewritten; `the_one_graph_table_finds_what_the_three_tables_synced` (src/offline/driver.rs, `graph_one_table`) syncs a store under the three tables' ids over a scripted Graph, a message, a contact and an event, then loads the one table and syncs again: mail and contacts resume from their stored delta links, the calendar lists the event it binds and reads nothing, no collection is created. On the old code it failed at the load of the new shape; with the contacts sugar renamed `msgraph-contact` both fail. Also `a_graph_domain_is_true_or_a_table`, `a_graph_table_names_its_domains`, nine refusals in `a_removed_key_points_at_the_migration_notes`, and every fixture moved to the one table, source names kept (src/offline/capability.rs, src/offline/driver.rs, src/cli/check.rs, tests/common/mod.rs, tests/msgraph.rs).
- config.sample.toml and README: the Graph section as one table. CHANGELOG: a **BREAKING** Changed entry and an upgrade line. MIGRATION.md: a before and after for each refused spelling.

## Left

- The live Graph suite (tests/msgraph.rs, `#[ignore]`) is rewritten, not run.
- The jmap-sources change assumes Graph's split in its J3; JMAP should follow this shape once it lands.

## Capabilities moved

- [sync](../spec/sync.md): *One Graph table keeps the source ids of the three it replaced* added; *A backend under the account is a source named after its protocol*, *The configuration refuses every key it does not read*, *The account group is the family's*, *Microsoft Graph is a first-class source*, *A Graph contacts source syncs the address book* and *A source's backend declares the kind it syncs* modified.
