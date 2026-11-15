---
cairn: log
change: a-removed-key-is-refused-as-the-file-is-parsed
date: 2026-11-15
---

# A removed key is refused as the file is parsed

Owner decision 2026-11-15: removed keys are refused by serde, at config time, as in the sibling apps (calendula). neverest refused them in `AccountConfig::validate`, so only in the accounts a command opened.

## What landed

- src/config.rs: a `removed_key!` macro declares a type no value deserializes into, its error the message `reject_removed_keys` printed; `left`, `right`, the account-level `collection` (alias `mailbox`), `folder`, `envelope`, `store.retention`, `store.hydration`, `collection.namespace` and the flat `smtp.login` and `smtp.password` take one. `RemovedKey`, the three `reject_removed_keys` and `declares_namespace` are gone.
- The `collection.namespace` message no longer lists the sources carrying it, and the `smtp` one names `smtp.sasl.*` whatever table it sits in: the parse error points at the key's line.
- `a_removed_key_points_at_the_migration_notes` now requires the parse to fail; three tests reading the refusal from `validate` read it from the parse, and two driver fixtures still written with `left.imap.server` use `imap.server`.

## Capabilities moved

- [sync](../spec/sync.md): *The configuration refuses every key it does not read* says when.
