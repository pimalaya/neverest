---
cairn: log
change: the-family-connection-keys
date: 2026-11-15
---

# The family connection keys

Lines N9 and N10 of the CLI alignment round of 2026-11-15 (audit findings F39, F40; decision D13). Every key is added, none renamed or removed.

## What landed

- Cargo.lock: pimalaya-config 0.5.0 to 0.5.1. Cargo.toml: the direct `shellexpand` dependency goes.
- src/config.rs: `store.root` and `tls.cert` read through `opt_shell_expanded_path` (`~` and `$VAR`); the tilde-only helper and its TODO are gone. `store.purge-after` and `store.index-timeout` are `Option<Duration>` read through `pimalaya_config::duration::option`; `HumanDuration` is gone.
- src/config.rs, src/account.rs, src/dav/client.rs: `carddav.home` and `caldav.home` (`DavEndpoint`), `server` optional; a home set is connected to and listed, nothing discovered. `auth = "none"`.
- src/config.rs, src/imap/client.rs: `imap.sasl-ir`, `imap.id.auto`, `imap.id.fields` (`auto_id`); `ImapClient::open` takes the resolved `ImapAccount`, reconnects included.
- src/config.rs, src/account.rs and every client: `proxy.url`, `proxy.username`, `proxy.password` at the account, under every backend table and under `smtp`, resolved once per run; Google Calendar's first stream is opened by neverest, io-gcal's connect taking no proxy.
- config.sample.toml documents each key.
- Tests: `a_variable_in_a_path_is_expanded_at_deserialize` and `a_duration_reads_seconds_or_a_unit_and_round_trips` (each failing on the old helpers), `the_family_connection_keys_load`, `a_proxy_table_refuses_an_unknown_key`, the proxy, home set and `ID` tests of `account::connection_tests`, `a_configured_home_set_is_taken_without_discovery`.

## Left

- With no proxy configured, every connection now reads `all_proxy` and `https_proxy`, as the siblings do; only Google Calendar's did before.
- No test reaches a proxy or an `ID` exchange on the wire: they are io-imap's and pimalaya-stream's, neverest only passing them on.

## Capabilities moved

- [sync](../spec/sync.md): *A DAV source may name its home set*, *An IMAP source takes the SASL-IR and ID quirks* and *A connection goes through the configured proxy* added; *A path key expands at deserialize, never at a call site*, *A run reclaims retained items on a schedule* and *The store's owner builds its search index* amended.
