---
cairn: change
id: discovery-wizard
status: landed
created: 2026-08-07
---

# The wizard opens on a welcome banner, asks for an email, and proposes discovered configurations

## Why

Neverest's wizard predates Himalaya's and diverged from it. It prompts an
account name first, then an email, then runs PACC / Mozilla autoconfig / RFC
6186 SRV **in series** keeping the first non-empty result, and finally hands
that single guess to the generic `pimalaya-cli` IMAP / JMAP wizards, which
re-ask host, port and encryption. Three consequences:

- the user has no idea what neverest is about to do, or what it will write
  where, before the prompts start;
- a serial first-wins discovery hides the other mechanisms' answers: the user
  never sees (nor picks between) what was actually found;
- the JMAP branch is offered even though no JMAP backend exists in this build,
  so the wizard can produce a config that `client::open` refuses.

Himalaya solved the same problem: a stderr welcome banner, one endpoint prompt,
io-pim-discovery's **parallel** compose fan-out, and one selectable entry per
reachable service. User feedback on Himalaya's prompt label ("Email, server or
URL:") is that it is misleading, so neverest takes the flow but asks for an
email only.

## What (design)

The wizard becomes: banner, one email prompt, a discovered-configuration list,
then the per-backend credential prompts and a live connection test.

- **Welcome banner** on stderr (mirroring Himalaya's, worded for sync): what
  neverest is, what the wizard does, where the documented sample lives.
- **One prompt, an email address.** A bare domain is still accepted (it is
  synthesized as `@domain`), but the label names the email only. Server URLs
  and folder paths are not neverest inputs: a side is always a remote, and the
  local side is the implicit pimdir store.
- **The account name is derived**, not prompted: the first label of the email
  domain (`clement.douin@posteo.net` -> `posteo`), as in Himalaya. The user
  renames the TOML table by hand.
- **Parallel discovery** through `DiscoveryComposeClientStd::compose_all_within`
  (fixed provider rules, PACC, Mozilla autoconfig, RFC 6186 SRV) under an 8s
  fan-out deadline, so one firewalled endpoint cannot stall the wizard. Every
  reachable service becomes one selectable entry carrying the authentication
  capabilities it advertised; the concrete mechanism is picked once the entry
  is chosen. `NEVEREST_DNS_RESOLVER` overrides the resolver, else the system
  resolver, else `1.1.1.1`.
- **Only compiled-in backends are offered**: IMAP + SMTP, plus the Microsoft
  Graph API on a detected Microsoft account. JMAP and Gmail are not proposed
  (no backend), instead of being offered and failing at open time. A Google
  account is configured through IMAP + SMTP with a token mechanism.
- **IMAP credentials are probed, not guessed**: an unauthenticated CAPABILITY
  probe narrows the SASL mechanism list to what the server advertises (falling
  back to the discovery-advertised list when the probe fails). Secrets go
  through the shared `pimalaya-cli` keyring / OAuth-broker pickers, so the
  generated config reads a password or a token from a command rather than
  storing it raw.
- **SMTP is configured when discovery found a submission endpoint**, as in
  Himalaya: "Use the same credentials for SMTP?" reuses the IMAP login and
  password, otherwise they are prompted again. Neverest's `smtp` channel
  authenticates with LOGIN only, so reuse is offered only when the IMAP
  mechanism carries a login + password pair; otherwise the wizard says so and
  prompts a dedicated pair (a blank login means an unauthenticated relay).
- **Every configured connection is tested** before the config is written
  (IMAP, then SMTP, then Graph), so a bad credential stops the wizard instead
  of yielding a config that cannot connect.
- **Remote to local only.** The wizard writes `left` plus the implicit store,
  never `right`: a remote-to-remote mirror stays a hand-written config, which
  is what the spec already says the wizard produces.
- **`neverest configure`** re-runs the same flow over an existing account
  (seeding the email prompt from the current side) and preserves everything the
  wizard does not own: the `default` flag, `store`, `mailbox`, `message`,
  `connections`, and a `right` side when one is configured by hand.

Superseded modules are deleted: `wizard/pacc.rs`, `wizard/autoconfig.rs`,
`wizard/srv.rs` (the serial mechanisms, now one compose call) and
`wizard/account.rs` (converters from the generic `pimalaya-cli` IMAP / JMAP
wizard answers, which the wizard no longer uses).

## Dependencies

`compose_all_within` landed in io-pim-discovery 0.4, published the same day, so
the dependency is bumped `0.3` -> `0.4` and neverest stays on released crates
(no git patch, unlike Himalaya). The `pimalaya-cli` `jmap` feature is dropped
(its JMAP wizard is gone) and `smtp` added; only its keyring / token pickers,
prompts and spinner remain in use.

## Out of scope

- Mailbox aliases: neverest's `mailbox.alias` is display-only and sync ignores
  it, and its backend keys differ from Himalaya's (the Graph adapter keys
  mailboxes by display name, not by well-known id), so the wizard writes none.
- A JMAP or Gmail wizard branch: they return when their backends do.
- Remote-to-remote setup, and any change to the sync engine.
