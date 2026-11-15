---
cairn: change
id: a-removed-key-reads-the-family-sentence
status: landed
created: 2026-11-15
---

# A removed key reads the family sentence

## Why

Audit finding F41 of the CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15): the family refused removed keys in six sentence shapes. pimalaya-config 0.5.1 (line L7) ships one, `toml::Removed` and `removed_key!`: "The `x` key is removed, <why>: <what to write instead>, see MIGRATION.md". neverest's own `removed_key!` wrote "`x` are gone: … See <URL of MIGRATION.md>".

## What

- Every removed-key type of src/config.rs is declared by `pimalaya_config::removed_key!`: `left`, `right`, the account-level `collection` (alias `mailbox`), `folder`, `envelope` (tables), `collection.namespace`, `store.retention`, `store.hydration`, `smtp.login`, `smtp.password` (keys). The pairs read as one type each before (`left` and `right`, `store.retention` and `store.hydration`, `smtp.login` and `smtp.password`) take one type a key, so each sentence names the key written.
- neverest's own macro and `MIGRATION_NOTES` go.
- The tests matching the old wording read the new sentence.

No key is renamed, removed or added: the same keys are refused, in other words.

## Decisions

- **`mailbox` reads as `collection`**: the alias shares the type, so its refusal names `collection`. Refusing `mailbox` by its own name is ledger line N8.
