---
cairn: change
id: the-family-connection-keys
status: landed
created: 2026-11-15
---

# The family connection keys

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, lines N9 and N10; audit findings F39 and F40, decision D13) found neverest reading less than its siblings:

- F39, D13: `store.root` and `tls.cert` expanded `~` alone, through a helper of neverest's own whose TODO waited for pimalaya-config's; the four readers expand `~` and `$VAR`.
- F40: durations were strings only (`"90d"`), where carillon and himalaya take integer seconds; DAV had no `home` (cardamum, calendula) and no `auth = "none"`; IMAP had no `sasl-ir`, `id.auto` nor `id.fields` (himalaya, carillon); no `proxy.*` (cardamum, himalaya, carillon).

Every key here is added; none is renamed or removed (brief of 2026-11-15: the live tester's configurations use the current keys).

## What

- `store.root` and `tls.cert` read through pimalaya-config's `opt_shell_expanded_path`; the local helper and its TODO go, and so does the direct `shellexpand` dependency.
- `store.purge-after` and `store.index-timeout` read through `pimalaya_config::duration::option` (pimalaya-config 0.5.1): an integer of seconds or `"90d"`; `HumanDuration` goes.
- `carddav` and `caldav` take `home`, the home set as a URL, `server` becoming optional; `auth = "none"`.
- `imap` takes `sasl-ir` and `id.auto`, `id.fields` as himalaya and carillon, neverest's canned values for `name`, `version`, `vendor`, `support-url`.
- `proxy.url`, `proxy.username`, `proxy.password` at the account, under every backend table and under `smtp`; the closest table wins, then the account's, then the environment's `all_proxy` and `https_proxy`.

## Decisions

- **An absent proxy reads the environment**, as cardamum, carillon and himalaya do (pimalaya-stream's `Proxy::System`). Neverest opened every connection directly but Google Calendar's, which io-gcal opened through the environment already: a person with `all_proxy` or `https_proxy` set now has every connection go through it.
- **Google Calendar's stream is opened by neverest**, io-gcal's own `connect` taking no proxy.
- **A home set skips discovery**: the session connects to it and lists there, `server` ignored, as cardamum does. A DAV table naming neither fails when the account resolves, naming both keys.
- **Durations are written back as pimalaya-config writes them**: the largest unit dividing it evenly (`"90d"`, `"45s"`).
