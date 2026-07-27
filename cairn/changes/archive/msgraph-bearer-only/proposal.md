---
cairn: change
id: msgraph-bearer-only
status: landed
created: 2026-08-07
---

# Microsoft Graph auth collapses to a bearer token command

## Why

The maintainer-approved layering decision is that **neverest must not know
OAuth**. The `store-owner` change ported three OAuth flows into
`src/msgraph/auth.rs` (device-code with a `tokens.json` refresh-token
persistence, client credentials by secret, client credentials by RFC 7523
certificate assertion) on top of io-oauth. That whole surface belongs to ortie,
the Pimalaya OAuth CLI, which serves cached and auto-refreshed access tokens
behind a command. The device-code flow is also interactive (it prints a
verification URI and waits), which has no place in a triggered headless sync
run.

## What (design)

Neverest keeps exactly one Graph auth mode: a bearer access token resolved
through the standard secret-command idiom, mirroring the Gmail side
(`gmail.auth.token`) and the IMAP password sources.

- `MsgraphAuthConfig` becomes a struct with a single `token: Secret` field, so
  the TOML shape is `<side>.msgraph.auth.token.raw` /
  `<side>.msgraph.auth.token.command`, identical to the Gmail block. The three
  flow tables (`auth.bearer`, `auth.device-code`, `auth.client-credentials`,
  `auth.client-credentials-cert`) are deleted.
- The command resolves to a `SecretString` at client open, once per opened
  client, directly in `client::open` (no dedicated `msgraph::auth` module
  remains; per naming-002 the module dies with its code).
- `src/msgraph/auth.rs` is deleted: device-code flow, `tokens.json` 0600
  persistence, both client-credentials flavors, every io-oauth usage.
- Deps: io-oauth is dropped; base64 loses its last user and is dropped too.
- `client::OpenContext` existed only to hand the Graph device flow its
  `tokens.json` location (and the account name for its interactive prompt); it
  is removed and `open` / `init` / `Pool::open` take the side config alone.
- Docs point OAuth setup (device sign-in, client credentials, certificates) at
  ortie; the sample shows
  `token.command = ["ortie", "token", "show", "--auto-refresh", "<account>"]`
  as the example of "any command printing a valid bearer token".

## Out of scope

The Graph client adapter itself (delta enumeration, bodies, pushes, sendMail)
is untouched: it already takes a resolved token.
