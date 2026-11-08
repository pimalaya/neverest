---
cairn: log
change: one-account-for-every-domain
date: 2026-11-08
---

# One account for every domain

The wizard adopts the flow of the Pimalaya Android app's onboarding, approved by the owner for the CLIs on 2026-11-08, after himalaya's the-wizard-picks-and-asks-once of the same day, whose behaviour and requirement names it mirrors. Neverest's domains are mail, sending, contacts and calendar, and the wizard now writes one account holding one source per domain it set up.

## What landed

- Dependencies: io-pim-discovery 0.9 to 0.11 from crates.io, same features (pacc, autoconfig, rfc6186, stream, rfc6764 with `dav`). No compile break: neverest never built a grant. rfc8414 and rfc9728 stay off, their fetches running past the deadline. pimalaya-cli patched to the local checkout (`[patch.crates-io]`, path ../cli) for `keyring::prompt_password`, `keyring::prompt_store`, `review::prompt` and `ortie::sign_in`; the lock takes pimalaya-cli 0.3.1 from the path, moves pimalaya-stream to 0.3.1 and io-proxy to 0.1.1, and adds psl and psl-types.
- src/wizard/search.rs asks for IMAP, SMTP, CardDAV, CalDAV and the provider API services (Gmail, People, Calendar, Graph mail, contacts and calendars) and answers the raw configs.
- src/wizard/plan.rs (new, pure): routes per domain (protocol, server, sign-in), plain TCP and non-https HTTP servers dropped, deduplicated, ranked (provider API first, implicit TLS before STARTTLS, OAuth first only with `ortie` on the PATH), Auto per domain, sending resolved against mail, the review lines (`Nothing found`, `Not set up`, `with mail's password`), the change lists (`Leave out`, `No sending`), the host grouping and the sign-in groups.
- src/wizard/server.rs (new, pure): a server URL typed for a domain (imaps/imap, smtps/smtp, https/carddav(s), https/caldav(s)), `http://` refused unless the host is this machine; `parse_any` reads the first `Server URL:` after nothing was discovered, `https://` setting up contacts and calendar alike.
- src/wizard/signin.rs (new): sign-ins grouped by identity, the password typed once and tested at once with `check`'s per-endpoint test (src/cli/check.rs `list`, now public) or the channel test, `prompt_store` under `neverest-<account>-<source>`, the reuse question for another host, the refusal menu (leaving mail out takes sending with it), OAuth through `ortie::sign_in` (account `neverest-<account>`, suffixed with its first service when several grants are needed) or the broker list. Domains signed in survive a return to the review.
- src/wizard/imap.rs and src/wizard/smtp.rs replace imap_smtp.rs: the IMAP mechanism from the CAPABILITY probe (SCRAM-SHA-256, PLAIN, LOGIN; OAUTHBEARER else XOAUTH2), SMTP PLAIN or XOAUTH2. dav.rs and msgraph.rs are gone: the sign-in builds every source block, user id `me`, DAV Basic or Bearer.
- src/wizard/settings.rs (new): the collections the test listed, in a `prompt::items` multi-select, mail starting without the `junk` and `trash` roles; all picked writes nothing, otherwise the unpicked ids become `collection.filter.exclude`.
- src/wizard/discover.rs: the review loop, the changes, `Server URL:` when nothing was discovered (an empty answer stops with the old message), the account assembled from every signed-in domain. src/cli/configure.rs settles the free account name before the sign-ins (`discover::run` takes the closure).
- src/config.rs: `AccountConfig::set_direct_backend` replaces `with_source` and `set_direct_source` (which overwrote the account `smtp` per source); `SourceBackendConfig::collection_mut`; the render order reads mail, `smtp`, contacts, calendar; `collection.create` and `collection.delete` are no longer written beside a filter when they hold their defaults. src/account.rs: `SourceAccount::resolve` and `SmtpAccount::resolve` open to every wizard build. `DavKind::protocol`, used by the old wizard alone, is gone.
- README.md, config.sample.toml, CHANGELOG.md and the welcome say what the wizard now does.

## Decided differently from the brief

- Sending over a Gmail API mail source is the provider's SMTP channel under the same grant, not "through mail": neverest's `gmail` source sends nothing (only Graph mail sends, through `sendMail`), so "through mail" would have left every queued send pending. The Ortie hand-off of a provider account covers `gmail`, `smtp`, `gpeople` and `gcal` at once.
- Graph mail sends through mail with no other choice: an account-level `smtp` table cannot complete a source that sends by itself (`attach_send_channel`), and sending is not set up without mail.
- The sign-in test is the whole of `check`'s per-endpoint row, the listing included, and its collections feed the settings: the Graph and Google clients open a TLS stream and send nothing until asked, so opening alone would pass a wrong token. A listing that fails is a refusal, with `Leave <domain> out`, never a silently skipped setting.
- No sugar name collides: each domain's protocols map to keys of their own, so the explicit `sources.<name>` form is never needed.
- When nothing is discovered, the first `Server URL:` reads its domain from the scheme, and `https://` sets up contacts and calendar from one DAV entry point; a sending URL is refused there, sending needing mail.

## Verified

- `cargo test --all-features`: 546 passed (521 unit tests), 68 server tests ignored as before. `cargo clippy --all-features --all-targets -- -D warnings` clean. `cargo check --no-default-features`, with rustls-ring alone, with rustls-ring and wizard, each backend feature alone (imap, smtp, dav, msgraph, gpeople, gcal, gmail, search) and the wizard with each backend: all build with no warning.

## Left

- No interactive run against a server or Ortie: the network and the keyrings were out of bounds. Ortie's `configure --service` was coded against pimalaya-cli's contract.
- pimalaya-cli's patch moves to a git rev at push time.
- The server suites were not run.

Spec: sync (modified: The wizard configures one channel from the account's credentials, The wizard discovers in parallel and proposes what it found, The generated configuration is a dotted document; added: Each domain gets its best option without a question, The review lists each pick in plain words, Nothing discovered asks for a server URL, A typed server URL names its protocol by its scheme, The sign-in mechanism is picked, not asked, Sign-ins are grouped by identity, A password is typed once, tested, then stored, OAuth hands off to Ortie, A refusal offers a way on, The collections to sync are picked last).
