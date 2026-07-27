---
cairn: change
id: dotted-config-document
status: landed
created: 2026-08-07
---

# The generated configuration renders like Himalaya's

## Why

The wizard serialized its result with `toml::to_string_pretty`, which promotes
every nested struct to its own table header and writes every defaulted field.
A Graph account came out as nine table headers, four of them empty, buried
under permission and TLS defaults nobody typed:

```toml
[accounts.outlook]
default = true

[accounts.outlook.left.msgraph]
user-id = "me"
alpn = ["http/1.1"]

[accounts.outlook.left.msgraph.tls.rustls]

[accounts.outlook.left.msgraph.auth.token]
command = "..."

[accounts.outlook.left.msgraph.mailbox]
create = true
delete = true
...
```

Himalaya solved this: `pimalaya_config::toml::to_string` renders one
`[accounts.<name>]` header per account with every field as a dotted key below
it (empty tables write nothing), and its config carries `skip_serializing_if`
predicates so a generated document holds only what the wizard decided. Neverest
uses the same config crate and the same wizard flow, so it should produce the
same document.

## What (design)

- **Rendering**: both serialization sites (`Config::write` and the wizard's
  stdout `GeneratedConfig`) go through `config_toml::to_string`. The account
  name is the only table header; everything else is dotted.
- **Defaults are omitted**, through Himalaya's `is_default` predicate: the
  account `default` flag when false, the per-side `mailbox` / `flag` /
  `message` permission triples and `pool-size`, the account `mailbox.filter`
  and `mailbox.alias`, the HTTP-backend `alpn` (JMAP, Gmail, Graph) and the
  IMAP / SMTP `starttls`. `StoreConfig`, `MessageSyncConfig` and `TlsConfig`
  need nothing: their fields are already optional, so they render as empty
  tables the renderer drops.
- Deserialization is untouched: every skipped field keeps its `#[serde(default)]`,
  so an omitted key reads back as the same value it would have written.

## Out of scope

- The `user-id` field, which stays written even at its `me` default, as in
  Himalaya: it names the mailbox the account talks to and is worth seeing.
- `config.sample.toml`, which documents every field on purpose and is not
  machine-generated.
