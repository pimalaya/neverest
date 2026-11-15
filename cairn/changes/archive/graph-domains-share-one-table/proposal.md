---
cairn: change
id: graph-domains-share-one-table
status: landed
created: 2026-11-15
---

# Graph's domains share one table

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, line N7; audit finding F37) found neverest the one Pimalaya CLI writing Microsoft Graph as three backend tables, `msgraph` (mail), `msgraph-contacts` and `msgraph-calendar`, where carillon, cardamum and calendula read one `msgraph` table. The owner ruled for one table, `msgraph-contacts` and `msgraph-calendar` refused by name, the design reviewed before any code.

The three tables each repeat one session (`user-id`, `tls`, `alpn`, `auth`, `proxy`): one host, one token, one user. They were split on purpose (a-graph-source-declares-its-domain, 2026-09-07) so the kind a source syncs falls out of the table it writes and no `kind` value is ever configured. That reason survives the change: a domain table under `msgraph` names the kind as the hyphenated key did.

**Hard constraint: an existing store keeps working.** A source's name is its pimdir source id (`<account>/<name>`), every collection id and binding derives from it, and renaming one orphans them (config.sample.toml, "RENAMING ONE ORPHANS THEM"). The sugar names a source after its table, so today's stores hold Graph data under the source ids `msgraph`, `msgraph-contacts` and `msgraph-calendar` (`work/msgraph-contacts/<folder>`), and a named source (`sources.people.msgraph-contacts`) under its own name. The first run of the new build must find every one of them.

## How neverest decides the kind today

- src/config.rs: `SourceBackendConfig` has three Graph variants, `Msgraph`, `MsgraphContacts`, `MsgraphCalendar`, each holding the same `MsgraphConfig` (session plus the per-source `collection`, `flag`, `item`, `pool-size`, `proxy` the `source_config!` macro splices in).
- `AccountConfig::sources()` names a sugar source by `SourceBackendConfig::protocol()`: `msgraph`, `msgraph-contacts`, `msgraph-calendar`. `SourceTable` reads a named source the same way.
- src/account.rs maps the variant to `GraphKind::{Mail, Contacts, Calendar}`; src/offline/capability.rs, src/offline/driver.rs, `carries_mail`, `sends_natively` match the variant. Nothing in the store names a table: the store holds source ids, collection ids, kinds (media types) and checkpoints.

So the change can stay in the configuration layer: the parse produces the same three variants under the same ids, and nothing past `AccountConfig::sources()` moves.

## Options considered

**A. One `msgraph` table, one table a domain under it (`mail`, `contacts`, `calendar`), the sugar keeping the three ids (recommended).** The session stays in `msgraph`; each domain table holds that domain's source keys or is written `true`. The sugar makes one source a domain, named as the table it replaces; a named source holds exactly one domain and keeps its name. Every id is kept, no store migration, the engine untouched. The names are pimdir's capability domains (`mail.message.add`, `contacts.*`, `calendar.*`), `GraphKind`'s variants and the old suffixes (`msgraph-contacts` becomes `msgraph.contacts`).

**B. As A, with carillon's nouns `msgraph.mailbox`, `msgraph.addressbook`, `msgraph.calendar`.** Closest in spelling to carillon. Rejected: carillon's keys are strings naming the one collection a watcher follows, so the same key would hold a different type and meaning; and `<backend>.mailbox` is the v0.2 alias of `collection` that N8 (the-mail-spellings-are-refused) refuses by name, so a v0.3 `msgraph.mailbox.filter` would fail as an unknown field of the new table instead of in the family sentence.

**C. One `msgraph` source syncing every domain under one source id.** Rejected: one source of several kinds breaks "A source's backend declares the kind it syncs" and the engine's one kind a source; the ids `msgraph-contacts` and `msgraph-calendar` would vanish, orphaning every contact and calendar binding unless the store were rewritten, which the owner's constraint forbids and pimdir gives no rename for.

**D. Named sources holding several domains, ids derived (`<name>`, `<name>-contacts`, `<name>-calendar`).** Rejected: a named contacts source `sources.people.msgraph-contacts` would become `people-contacts`, orphaned, unless a lone domain kept the bare name, in which case adding a second domain renames the first. The map key stays the source id, one domain a named source.

**E. A value selecting the domains (`msgraph.domains = ["mail", "contacts"]`, or `kind = "contacts"`).** Rejected: a kind configured by value, which the spec forbids, and two places (the list and the domain tables) to keep in step.

**F. Mail implicit: `msgraph` alone keeps syncing mail, `contacts` and `calendar` opt in.** Every v0.3 mail configuration would keep loading, but `msgraph` would mean "mail unless told otherwise", there would be no way to write a contacts-only table without a `mail = false`, and the per-source keys of mail would sit beside the session while the others sit in domain tables. Rejected for the asymmetry; A refuses the bare table by name instead, naming `msgraph.mail = true`.

**G. Per-source keys shared at the `msgraph` level for every domain.** Rejected: a mail include list would filter out every contact folder, the reason "A collection filter belongs to the source".

## Recommendation: option A

### The shape

Sugar, under the account:

```toml
[accounts.work]
msgraph.auth.token.command = ["ortie", "token", "show", "-a", "microsoft"]
msgraph.user-id = "me"
msgraph.mail.collection.filter.include = ["Inbox", "Sent Items"]
msgraph.contacts.item.delete = false
msgraph.calendar = true
```

- `msgraph` holds the session: `user-id`, `tls`, `alpn`, `auth`, `proxy`.
- `msgraph.mail`, `msgraph.contacts`, `msgraph.calendar` each hold that domain's source keys: `collection` (`filter`, `create`, `delete`, `rename`, `capped`), `flag`, `item`, `pool-size`. `= true` takes their defaults; `= false` reads as absent. At least one domain is required.
- Each named domain is one source, named as the table it replaces: `mail` is `msgraph`, `contacts` is `msgraph-contacts`, `calendar` is `msgraph-calendar`.

Explicit, a named source or target holding one domain, the name being the source id:

```toml
sources.outlook.msgraph.auth.token.command = ["ortie", "token", "show", "-a", "microsoft"]
sources.outlook.msgraph.mail = true
sources.people.msgraph.auth.token.command = ["ortie", "token", "show", "-a", "microsoft"]
sources.people.msgraph.contacts.item.delete = false
```

The sugar above is exactly `sources.msgraph.msgraph.mail…`, `sources.msgraph-contacts.msgraph.contacts…` and `sources.msgraph-calendar.msgraph.calendar…`, each with the session copied: expanding it by hand stays a no-op on the store. A send channel is unchanged: Graph mail sends natively, and `smtp` beside a contacts or calendar domain is refused as today.

### Every source id kept

| Today | Rewritten | Source id |
|---|---|---|
| `msgraph.*` (mail) | `msgraph.<session>`, `msgraph.mail.*` or `= true` | `msgraph` |
| `msgraph-contacts.*` | `msgraph.<session>`, `msgraph.contacts.*` or `= true` | `msgraph-contacts` |
| `msgraph-calendar.*` | `msgraph.<session>`, `msgraph.calendar.*` or `= true` | `msgraph-calendar` |
| `sources.<n>.msgraph.*` | `sources.<n>.msgraph.<session>`, `.mail` | `<n>` |
| `sources.<n>.msgraph-contacts.*` | `sources.<n>.msgraph.<session>`, `.contacts` | `<n>` |
| `sources.<n>.msgraph-calendar.*` | `sources.<n>.msgraph.<session>`, `.calendar` | `<n>` |

`targets.<n>` reads as `sources.<n>`. Three sugar tables whose sessions differ (two tokens, two users, two proxies) cannot be one table: they become named sources under the same names (`sources.msgraph-contacts.msgraph.contacts = true` with its own `auth`), which keeps the ids too. Nothing in the store is migrated: it holds no table name. The account's mode (arity, `one-way`, `retain`) is unchanged, the expansion yielding as many sources. Queued invitation intents naming `"source":"msgraph-calendar"` and frontends naming `work/msgraph-contacts/<folder>` keep working.

### Refused by name

In the family sentence (`removed_key!`), as the file is parsed:

- `msgraph-contacts` and `msgraph-calendar` tables, under the account, `sources.<name>` and `targets.<name>`: "The `msgraph-contacts` table is removed, Graph's domains sharing one `msgraph` table and its session: write `msgraph.contacts`, its `auth`, `user-id`, `tls`, `alpn` and `proxy` in `msgraph` itself, see MIGRATION.md" (`msgraph.calendar` for the other).
- `collection`, `item`, `flag` tables and the `pool-size` key directly in an `msgraph` table: "The `msgraph.item` table is removed, each Graph domain syncing with its own rights: write `msgraph.mail.item` for mail, see MIGRATION.md".

Refused at load, not removed keys:

- An `msgraph` table naming no domain (every v0.3 mail-only table): "The `msgraph` table names no domain: write `msgraph.mail = true` to sync mail as before, `msgraph.contacts` or `msgraph.calendar` for the others, see MIGRATION.md".
- A named source's `msgraph` naming two domains or more: "A source's `msgraph` table names `mail` and `contacts`, and a source syncs one domain: give each a source of its own, or write `msgraph` under the account, which names each after its domain".

### Output

`account list` names `msgraph` for every Graph source, `msgraph-contacts` and `msgraph-calendar` leaving `BACKENDS`, and an `account check` row's `backend` reads `msgraph` whatever the domain, as cardamum's and calendula's do; the row's `source` still reads `msgraph-contacts`. Both are `--json` changes.

### MIGRATION entry (next release)

```md
- **Microsoft Graph is one `msgraph` table**, as in cardamum, calendula and carillon: `msgraph-contacts` and `msgraph-calendar` are refused by name. The session keys (`auth`, `user-id`, `tls`, `alpn`, `proxy`) stay in `msgraph`; each domain's `collection`, `item`, `flag` and `pool-size` move under `msgraph.mail`, `msgraph.contacts` or `msgraph.calendar`, a domain with none written `= true`. Before: `msgraph.auth.token.command = [...]`, `msgraph.item.delete = false`, `msgraph-contacts.auth.token.command = [...]`, `msgraph-calendar.auth.token.command = [...]`. After: `msgraph.auth.token.command = [...]`, `msgraph.mail.item.delete = false`, `msgraph.contacts = true`, `msgraph.calendar = true`. An `msgraph` table naming no domain is refused: add `msgraph.mail = true` to sync mail as before. Under `sources.<name>` or `targets.<name>`, `msgraph-contacts.*` becomes `msgraph.*` with `contacts` alone, one domain a source. **Nothing in the store moves**: the sources keep their names (`msgraph`, `msgraph-contacts`, `msgraph-calendar`, or the name you gave), so the first run finds every collection and resumes where the last stopped. Tables naming different tokens or users stay apart as named sources under the same names: `sources.msgraph-contacts.msgraph.contacts = true` with its own `auth`. Under `--json`, `account list` and an `account check` row's `backend` read `msgraph` for every Graph source, where they read `msgraph-contacts` or `msgraph-calendar`.
```

### The regression test

Two tests, the first committed before any configuration code changes so it runs on today's code:

1. `the_graph_tables_keep_their_source_ids` (src/config.rs): a table of documents and the `(source id, GraphKind)` pairs `AccountConfig::namespaced("a")` gives: today's sugar (`msgraph.*`, `msgraph-contacts.*`, `msgraph-calendar.*`) and named forms (`sources.people.msgraph-contacts.*`, `targets.copy.msgraph-calendar.*`) give `a/msgraph` mail, `a/msgraph-contacts` contacts, `a/msgraph-calendar` calendar, `a/people` contacts, `a/copy` calendar. It passes on today's code; the change rewrites only its documents to the new shape, the expected pairs untouched, so the diff itself shows no id moved.
2. `the_one_graph_table_finds_what_the_three_tables_synced` (src/offline/driver.rs, beside `graph_heads`, over the scripted Graph server): run 1 syncs a temp store for account `a` through the three sources today's sugar built (`SourceConfig::new` of the three variants keyed `msgraph`, `msgraph-contacts`, `msgraph-calendar`, the mapping test 1 pins), an inbox of one message, the default contacts folder of one contact and one calendar of one event, mail and contacts answering a delta link `…$deltatoken=t1`. Run 2 loads the new-shape document (`msgraph.auth.token.raw`, `msgraph.mail = true`, `msgraph.contacts = true`, `msgraph.calendar = true`) and syncs over a script whose mail and contacts steps accept only GET of the stored `…$deltatoken=t1` link, answering no change, the calendar listing the same event at the same `changeKey`. It asserts the store's sources are exactly `a/msgraph`, `a/msgraph-contacts`, `a/msgraph-calendar`, its collections the three of run 1 with none added, each item bound once under its run 1 handle, the report showing no create, delete, push or fetch, and every script step consumed. On today's code it fails at the load (`msgraph.mail` is no field); on a wrong mapping (contacts under `msgraph`, a named source renamed) the source has no checkpoint, its first request is the initial delta URL and the script refuses it.

## Consequences

- JMAP: the live tester's jmap-sources change (J3, "three keys, the rule Graph follows") assumes Graph's split. Once this lands JMAP should follow it (`jmap.mail`, `jmap.contacts`, `jmap.calendar`, sugar ids `jmap`, `jmap-contacts`, `jmap-calendar`); no JMAP store exists yet, so nothing to keep. That change is not edited here.
- The live tester's rc files and tests/msgraph.rs (the live Graph suite) write `msgraph-contacts` and `msgraph-calendar`: both move with the code, the rc files by their owner.
- Credentials: three domain sources share one token command, spawned once a run already (*Credentials are resolved once per run* memoizes by command).

## Ruling

D16 of the CLI alignment plan, the owner on 2026-11-15: "refuse it, go with the design as written". The four points put to the owner are settled as the design wrote them:

1. Option A, the domains named `mail`, `contacts`, `calendar` (pimdir's domains), not carillon's `mailbox`, `addressbook`, `calendar`.
2. `= true` for a domain taking its defaults (a deserializer taking a bool or a table), `false` reading as absent.
3. `pool-size` per domain, as each source had its own pool; `proxy` at the session only.
4. `backend: "msgraph"` in `account check` rows and in `account list`, as the family reads it.

The refusal sentences, the source ids and the tests landed as written above; the implementation is in [the log](../../../log/2026-11-15-graph-domains-share-one-table.md).
