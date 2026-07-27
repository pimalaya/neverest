---
cairn: log
change: msgraph-bearer-only
landed: 2026-08-07
---

# Microsoft Graph auth collapsed to a bearer token command

Neverest no longer knows OAuth. The three Graph OAuth flows the `store-owner`
change ported in (RFC 8628 device-code with a `tokens.json` mode-600
refresh-token persistence, client credentials by shared secret, client
credentials by RFC 7523 certificate assertion) moved out to ortie, the Pimalaya
OAuth CLI, which serves cached and auto-refreshed access tokens behind a
command.

**Config** (`config.rs`): `MsgraphAuthConfig` collapsed from a four-variant
tagged enum to a single-field struct `{ token: Secret }`, so the TOML shape is
`<side>.msgraph.auth.token.raw` / `.command`, identical to the Gmail block and
the IMAP password sources. The three flow config structs, their tenant/scope
defaults and the now-unused `shell_expanded_path` helper are gone; the parsing
test is bearer-only and asserts a leftover flow table fails loudly.

**Auth module deleted** (`src/msgraph/auth.rs`): device sign-in prompt and
polling, `tokens.json` load/store with the 0600 file handling, both
client-credentials flavors, the RFC 7523 assertion signing and every io-oauth
usage. The token command resolves to a `SecretString` inline in `client::open`,
once per opened client, so no dedicated module remained (naming-002).

**Seam simplification** (`client.rs`): `OpenContext` (account name + store dir)
existed only to hand the device flow its `tokens.json` location; it is removed
and `open` / `init` / `Pool::open` take the side config alone, simplifying
`cli/init.rs`, `cli/check.rs` and `offline/driver.rs`.

**Deps**: io-oauth dropped; base64 lost its last user (the PEM-to-DER helper)
and is dropped too. io-imap and io-msgraph gained explicit local path patches:
the committed lockfile had resolved them to path copies whose unpublished APIs
(`fetch_bodies_stream`, `messages_delta`) neverest uses, but the patch entries
were missing from Cargo.toml, so a fresh resolution broke against crates.io.

**Docs**: the sample msgraph auth block now shows the ortie pattern
(`token.command = ["ortie", "token", "show", "--auto-refresh", "msgraph"]`,
worded as any command printing a valid bearer token) and states that OAuth
setup lives in ortie. CHANGELOG rewritten net-style: the Graph entry documents
the bearer-command contract, the interim tagged-flow breaking entry is gone.

Verified: all tests green (31 lib/bin, relay ignored as before), fmt clean,
clippy clean except the known pre-existing `type_complexity` in
`wizard/autoconfig.rs`.

Spec updated: `sync` (MODIFIED: "Microsoft Graph is a first-class side" now
states the bearer-command contract and the ortie delegation).
