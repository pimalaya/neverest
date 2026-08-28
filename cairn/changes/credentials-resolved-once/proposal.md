---
cairn: change
id: credentials-resolved-once
status: landed
created: 2026-08-28
---

# Credentials are resolved once per run, not once per connection

## Why

A configuration says where a credential comes from, not what it is: a
`password.command` is a `pass` or `gpg` invocation waiting to be spawned.
Neverest carried that unresolved form all the way down to the socket.
`client::open` took a `SourceConfig` and resolved its secret inside, and `Pool`
kept the configuration so that every lazily-opened connection could resolve it
again.

**It made connecting expensive.** An IMAP source at the default `-j 4` opens
four connections, each spawning the password command, and `gpg-agent` serialises
them. A posteo account with an IMAP source, a CardDAV one, a CalDAV one and a
send channel paid six `gpg` invocations per run, four of them concurrently, for
what is usually one or two distinct password entries.

**It made the wait invisible.** Resolution sat between the last configuration
read and the first transport log, in a code path that logged nothing at any
level, so a locked agent showed as several seconds of a silent terminal before
the first `Opened 1 connection(s)`.

**It made the boundary unstatable.** Nothing in the type system said where a
secret stops being a command and starts being a value, so the answer was
"wherever `open` is called", which is the connection layer. The protocol clients
below it were already right: `ImapClientStd::connect` takes a `Sasl`,
`DavClient::connect` a `WebdavAuth`, `GraphClient::connect` a `SecretString`.
The seam existed one level too low.

## What

- A runtime `Account`: the endpoints of one account with every secret resolved,
  built once per run. `SourceAccount` holds one endpoint's connect material
  (`ImapAccount`, `DavAccount`, `MsgraphAccount`, `SmtpAccount`), which is
  exactly the argument list of the backend's `connect`.
- `client::open` and `Pool` take a `SourceAccount` instead of a `SourceConfig`,
  so opening a connection spawns no process and reads no configuration.
- One `pimalaya_config::secret::SecretResolver` per resolution, memoizing the
  commands it spawns, so an account naming one password entry from its IMAP,
  SMTP, CardDAV and CalDAV tables reads it once.
- A resolution failure is kept per endpoint rather than failing the account, so
  a stale entry for calendars does not leave mail unsynced. It surfaces where
  the driver already reports a source that could not be opened.
- The wait is visible: a "Resolving credentials" spinner, and pimalaya-config
  logs each spawn at `debug` with the time it took.

## Not in scope

**No refresh.** An account is resolved once and never re-read, which is exact
for a one-shot run: a token whose lifetime is shorter than the process cannot be
refreshed from a value taken at startup. Neverest has no watch verb, so nothing
needs it. The credential is shaped so a resolver can be added beside the value
when a long-lived caller appears.

**No configuration change.** The TOML schema, the store format and the wizard's
questions are untouched: this moves where a configured command runs, not what
may be configured.
