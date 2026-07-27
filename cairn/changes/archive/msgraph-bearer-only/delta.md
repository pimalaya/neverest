---
cairn: change
change: msgraph-bearer-only
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: Microsoft Graph is a first-class side
An `msgraph` side SHALL open protocol-direct over io-msgraph (never through a
frozen aggregator): folders listed two levels deep (`Parent/Child` naming),
enumeration through the messages delta query carrying the `@odata.deltaLink`
as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh
full round; any other failure surfaces), the `Meta` tier served from the cached
delta rows (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME
content streamed into the blob store. Flags map to the IANA wire spellings
(`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`).
Auth SHALL be a bearer access token only, resolved through the standard
secret-command idiom (`auth.token.raw` / `auth.token.command`) once per opened
client; neverest SHALL NOT run any OAuth flow itself (no device sign-in, no
client credentials, no token persistence): acquiring and refreshing the token
is delegated to an external command, typically ortie. No token is ever logged.
Push scope is honest: flag changes push through `message_update` and deletes
through `message_delete`; appends, moves and content updates are rejected
(pull-only) and documented.

## REMOVED Requirements
