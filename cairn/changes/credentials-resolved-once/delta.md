---
cairn: change
change: credentials-resolved-once
---

# Delta

## ADDED Requirements

### Requirement: Credentials are resolved once per run
A run SHALL resolve every configured secret once, up front, into a runtime
account holding the values themselves, and SHALL open every connection from
that account. Nothing below that seam may spawn a process to authenticate: a
second connection to a side, whether opened eagerly for the connection budget or
lazily for a concurrent fetch, SHALL cost a handshake and no credential read.

Resolution SHALL memoize the commands it spawns, keyed on the command as the
configuration wrote it, so a configuration naming one password entry from
several tables SHALL spawn it once per run rather than once per table.

The key SHALL be the configured shape itself, a shell line or a program with
its arguments, compared as written and never across the two: a shell line and
the argv spelling that runs it through the platform shell SHALL resolve on
their own. Reading one as the other means guessing what a configuration meant,
and handing a credential to a field that did not ask for it is the failure that
guess would cause.

A credential that fails to resolve SHALL fail its endpoint, not the account: the
error SHALL be raised when that endpoint is read, and reported where a source
that could not be opened is already reported, so the account's other sources
still sync.

The wait SHALL be visible. A credential store answers in seconds when its agent
is locked, so the resolution SHALL be reported while it runs, and each spawned
command SHALL be logged with the time it took. Neither the resolved value nor
the command arguments SHALL be logged, a command line being free to carry the
secret itself.

An account is resolved once and never re-read within a run. This is exact for a
one-shot sync and SHALL NOT be relied on by a long-lived caller, which would
resolve a new account rather than refresh this one.

## MODIFIED Requirements

### Requirement: Microsoft Graph is a first-class source
An `msgraph` source SHALL open protocol-direct over io-msgraph (never through a
frozen aggregator): folders listed two levels deep (`Parent/Child` naming),
enumeration through the messages delta query carrying the `@odata.deltaLink`
as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh
full round; any other failure surfaces), the `Meta` tier served from the cached
delta rows (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME
content streamed into the blob store. Flags map to the IANA wire spellings
(`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`).
Auth SHALL be a bearer access token only, resolved through the standard
secret-command idiom (`auth.token.raw` / `auth.token.command`) once per run with
every other credential; neverest SHALL NOT run any OAuth flow itself (no device
sign-in, no client credentials, no token persistence): acquiring and refreshing
the token is delegated to an external command, typically ortie. No token is ever
logged. Push scope is honest: flag changes push through `message_update` and
deletes through `message_delete`; appends, moves and content updates are
rejected (pull-only) and documented.
