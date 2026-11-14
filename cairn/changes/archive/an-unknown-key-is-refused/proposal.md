---
cairn: change
id: an-unknown-key-is-refused
status: landed
created: 2026-11-14
---

# An unknown key is refused

## Why

The owner decided on 2026-11-14, family-wide, that a configuration refuses every key it does not read, at every level, so a typo anywhere fails loading naming the key; and that a key the app used to read and removed is refused by name, pointing at the migration notes, rather than by serde's generic unknown-field error.

Every neverest config struct already carried `deny_unknown_fields` but two. The document root (`Config`) took any key beside `accounts`. A source table (`sources.<name>`, `targets.<name>`) was `SourceConfig`, whose backend was `#[serde(flatten)]`: serde cannot deny unknown fields across a flatten, so a key misspelled beside the backend table (`sources.work.imap-typo`) loaded and was ignored, and a source naming two backend tables kept the first one the map yielded, the other dropped silently. The spec already said a source declares exactly one backend.

The removed keys were refused by name already (`left`, `right`, the account `collection`, `collection.namespace`, `store.retention`, `store.hydration`), but none pointed at MIGRATION.md; the v0.1 `folder` and `envelope` tables and the flat `smtp.login` and `smtp.password` of the git builds before v0.2.0 fell to the generic error; and `collection.namespace` on a backend written directly under the account (`imap.collection.namespace`) loaded and validated, the check walking the explicit `sources` and `targets` alone.

## What

- `Config` denies unknown fields: the root holds `accounts` and nothing else.
- `SourceConfig` deserializes through a private `SourceTable`, `deny_unknown_fields`, one optional field per backend table plus `smtp`, then `TryFrom` refuses a table naming no backend or two, naming both. The TOML a user writes is unchanged; `SourceConfig` keeps its `backend` field and serializes as before through `flatten`, which is only a problem when reading. `SourceBackendConfig` loses its unused `Deserialize`.
- `AccountConfig` gains `folder` and `envelope`, `SmtpConfig` gains `login` and `password`, all `RemovedKey`s refused by `AccountConfig::validate` with their replacement.
- Every removed-key refusal ends with `See https://github.com/pimalaya/neverest/blob/master/MIGRATION.md`.
- `collection.namespace` is looked for on every source, the sugar included.
- MIGRATION.md names the change and the flat SMTP keys' replacement.

## Decisions

- **Refused at validation, not at deserialization.** neverest already refused its removed keys from `AccountConfig::validate`, which every command (`sync`, `check`, `init`, `drain`, `index`, `conflict`) runs on each account it opens before anything connects; the new ones join that path rather than moving them all to deserializer errors as Calendula does. An account `-a` leaves out is not validated, so a removed key there does not fail the run, where an unknown key anywhere does.
- **`try_from`, not a custom `Deserialize`.** The derived `SourceTable` gives serde's own unknown-field error, listing the accepted keys, and the source keeps its Rust shape for every caller.
- **Only keys a release or the spec named.** Keys removed between two git builds and never released (`conflict.notify`, the Graph flow tables under `msgraph.auth`) stay generic unknown fields.
