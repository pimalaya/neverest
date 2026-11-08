---
cairn: change
id: one-account-for-every-domain
status: landed
created: 2026-11-08
---

# One account for every domain

## Why

The wizard writes one account with one source: it lists every reachable service, asks which one, then asks the SASL mechanism, the login, a keyring strategy and an entry name per credential, and whether SMTP reuses the IMAP credentials. A password is never typed into the wizard: the user stores it beforehand under a name they have to guess. A provider carrying mail, contacts and calendar gets one of the three, the other two written by hand. Discovery finding nothing stops the wizard.

The Pimalaya Android app's onboarding settled a shorter flow, approved by the owner for the CLIs on 2026-11-08: one input, the best option picked per domain, a review in plain words, one sign-in per identity, a password typed once, tested and stored by the wizard, settings last. Himalaya landed it the same day (its cairn change the-wizard-picks-and-asks-once); neverest's domains are mail, sending, contacts and calendar, in one account.

## What

- io-pim-discovery 0.9 to 0.11 (MX provider fallback, RFC 6764 roots), asked for the provider API services its rules carry (the Gmail API, the People and Calendar APIs, Microsoft Graph mail, contacts and calendars). rfc8414 and rfc9728 stay off: their fetches run past the deadline. pimalaya-cli patched to the local checkout for `keyring::prompt_password`, `keyring::prompt_store`, `review::prompt` and `ortie::sign_in`.
- A plan (src/wizard/plan.rs, pure): the options per domain, ranked (provider API first, implicit TLS before STARTTLS, OAuth first only with `ortie` on the PATH), Auto picked without a question, the review lines, the change lists, the sign-in groups.
- A typed server URL (src/wizard/server.rs, pure) per domain, its scheme naming the protocol, `http://` refused unless the host is this machine.
- Sign-ins grouped by identity (src/wizard/signin.rs): one Ortie hand-off per authorization server, one password per group of servers of one name, tested at once with `check`'s per-endpoint test (open, then list the collections with their roles), stored under `neverest-<account>-<source>`, a refusal offering a way on.
- Settings last: per source, the collections the test listed in a multi-select, all picked but the junk and trash roles for mail; the unpicked ones written as `collection.filter.exclude`.
- One account, one source per domain set up, through the direct-backend sugar; the mail source's `smtp` channel when sending goes over SMTP.

## Decided against the brief

- Sending over a Gmail API mail source is an SMTP channel under the same grant, not "through mail": neverest's `gmail` source does not send (only Graph mail does, through `sendMail`), so "through mail" would leave every queued send pending.
- Graph mail sends through mail with no alternative: an account-level `smtp` table cannot complete a source sending by itself.
- The sign-in test lists the collections: the Graph and Google clients open a TLS stream and send nothing until asked, so opening alone would pass a wrong token. A listing that fails is therefore a refusal, never a silently skipped setting.
