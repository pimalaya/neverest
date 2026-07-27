---
cairn: log
change: discovery-wizard
landed: 2026-08-07
---

# The wizard asks for an email and proposes what discovery found

The wizard now mirrors Himalaya's: a welcome banner, one email prompt, a list
of discovered configurations, then the credentials of the one picked. The old
flow (account name, then email, then PACC / autoconfig / SRV tried in series
keeping the first non-empty answer, then the generic `pimalaya-cli` IMAP / JMAP
wizards re-asking host, port and encryption) is gone.

**Discovery** (`wizard/search.rs`, new): one
`DiscoveryComposeClientStd::compose_all_within` call runs every mechanism in
parallel under an 8 second deadline, so a firewalled endpoint cannot stall the
prompts. Each reachable service becomes one `Discovered` entry carrying the
`AuthCaps` it advertised (password / bearer / OAuth grant folded onto three
axes), a provider short-circuit restricts IMAP + SMTP to a detected provider's
own configs, and the resolver is `NEVEREST_DNS_RESOLVER`, else the system
resolver, else `1.1.1.1`. Only what this build can open is proposed:
`retain_supported` drops the entries whose cargo feature is off, and Google
gets no proprietary entry at all (no Gmail backend) so it lands on IMAP + SMTP
with a token mechanism. Verified live against posteo.net: IMAP 993 + SMTP 465,
implicit TLS, password auth, in 8 seconds.

**IMAP + SMTP** (`wizard/imap_smtp.rs`, new): an unauthenticated CAPABILITY
probe narrows the SASL menu to what the server advertises (falling back to the
discovery-advertised list, logged not surfaced, when the probe fails), secrets
go through the shared keyring / OAuth-broker pickers, and the side is opened
through `client::open` as the connection test. The send channel is configured
from the discovered submission endpoint only: "Use the same credentials for
SMTP?" reuses the IMAP pair, otherwise a dedicated login and password are
prompted (a blank login being an unauthenticated relay). Because neverest's
`smtp` table authenticates with LOGIN, reuse is offered only for the password
mechanisms; a token-based IMAP account says so and prompts a pair instead. A
build without the `smtp` feature reports the discovered endpoint and drops it.

**Graph** (`wizard/msgraph.rs`, new): user id plus a bearer token secret (the
ortie pattern), tested through `client::open` before it is written.

**Entry point** (`wizard/discover.rs`, rewritten): banner, the single
`Email address:` prompt (a bare domain is synthesized as `@domain`), an account
name derived from the domain's first label, the configuration list, then a
one-side account (`left` + the implicit store, never `right`). Discovery that
finds nothing stops the wizard and points at `config.sample.toml` instead of
dropping into hand entry.

**`neverest configure`** (`wizard/edit.rs`, rewritten): the same flow, seeded
with the account's current email, replacing `left` and the send channel while
carrying over `default`, `store`, `mailbox`, `message`, `connections` and a
hand-written `right` (announced when present). A run that discovers no
submission endpoint keeps the channel already configured.

**Deleted**: `wizard/pacc.rs`, `wizard/autoconfig.rs`, `wizard/srv.rs` (the
serial mechanisms, now one compose call) and `wizard/account.rs` (converters
from the generic `pimalaya-cli` wizard answers, no longer used).

**Deps**: `io-pim-discovery` 0.3 -> 0.4 for `compose_all_within` (published the
same day, so no git patch was needed and neverest stays on released crates);
`pimalaya-cli` drops the `jmap` feature and adds `smtp`, keeping only its
prompts, keyring pickers and spinner.

**Docs**: README, `config.sample.toml` header and CHANGELOG rewritten around
the new flow (the README still described an account-name prompt and an m2dir
store root for the other side).

Verified: 42 tests green, fmt clean, clippy clean except the pre-existing
`incompatible_msrv` warning in `cli/sync.rs`, and every feature subset
(none / imap / msgraph / imap+smtp / msgraph+smtp / all) compiles warning-free.

Spec updated: `sync` (ADDED: "The wizard discovers in parallel and proposes
what it found"; MODIFIED: "Sides are remote backends only" now states the
wizard writes `left` only).
