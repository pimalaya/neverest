---
cairn: log
change: dotted-config-document
landed: 2026-08-07
---

# The generated configuration renders like Himalaya's

The wizard serialized through `toml::to_string_pretty`, which promotes every
nested struct to its own table header and writes every defaulted field: a Graph
account came out as nine headers, four of them empty, padded with permission,
TLS and ALPN defaults nobody typed. Both serialization sites now go through
`pimalaya_config::toml::to_string`, the renderer Himalaya uses, and the config
carries Himalaya's skip predicates.

**Rendering** (`config.rs`, `wizard/discover.rs`): `Config::write` and the
wizard's stdout `GeneratedConfig` both call `config_toml::to_string`, so the
account name is the only table header, every field below it is a dotted key,
empty tables write nothing, and a saved file is byte-for-byte the document a
redirected stdout prints.

**Defaults omitted** (`config.rs`): Himalaya's `is_default` predicate (plus
`is_default_http_alpn` for the ALPN list, whose default is non-empty) now skips
the account `default` flag when false, the per-side `mailbox` / `flag` /
`message` permission triples, `pool-size`, `mailbox.filter`, `mailbox.alias`,
the JMAP / Gmail / Graph `alpn` and the IMAP / SMTP `starttls`. The three
permission structs and `MailboxFilter` gained `PartialEq` for the predicate.
`StoreConfig`, `MessageSyncConfig` and `TlsConfig` needed nothing: their fields
are already optional, so they render as empty tables the renderer drops.
Deserialization is untouched, every skipped field keeps its `serde(default)`,
so omitting a key reads back as the value that was skipped.

What the reported Graph account renders as now:

```toml
[accounts.outlook]
default = true
left.msgraph.auth.token.command = "…"
left.msgraph.user-id = "me"
```

Verified: a new `config` test pins that document (one header, dotted keys, no
defaulted value); an IMAP + SMTP account renders the same way (server, SASL
credentials and the smtp channel, nothing else); 43 tests green, fmt and clippy
clean except the pre-existing `incompatible_msrv` warning in `cli/sync.rs`.

Spec updated: `sync` (ADDED: "The generated configuration is a dotted
document").
