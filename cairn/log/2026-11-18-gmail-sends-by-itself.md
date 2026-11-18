---
cairn: log
change: gmail-sends-by-itself
date: 2026-11-18
---

# Gmail sends by itself

The owner's ruling of 2026-11-18: a Gmail account sends through Gmail by default, the `smtp` table optional, a submission channel of its own when given. Until now a Gmail source without `smtp` declared `mail.submit` `none` and its sends stayed pending.

## What landed

- src/config/source.rs: `sends_natively` answers for Gmail as for Graph mail.
- src/config.rs: the account `smtp` table still lands on a Gmail sugar source, which then sends through it; Graph mail never takes it, as before. Test `gmail_sends_by_itself_or_through_the_account_smtp`.
- src/gmail/client.rs: `fresh_stream` opens the API session a new connection; `send_raw` posts the raw body to `users.messages.send` (io-gmail 0.9's `GmailMessageSend`, 100 quota units), sent again on a 429 alone. Test `a_send_posts_the_raw_message_over_a_fresh_stream`.
- src/sync/driver/send.rs: the native pick opens a fresh connection before the first Gmail send, so none goes out over one Gmail closed while it idled; a connection it cannot open names the source under `unreached`, the intents pending.
- src/sync/submit.rs: `Link::Gmail` and `gmail_disposition`, Graph's verdicts but a 403 left pending (a token without the send scope or a domain policy is the user's to fix; Gmail's rate-limit 403 is waited on and read as 429). A failure with no status is unconfirmed, a 2xx that does not parse included, except a request that could not be built. Test `a_gmail_send_failure_is_judged_by_its_status`. The send-channel `cfg` gates take the `gmail` feature.
- src/sync/capability.rs: a Gmail source declares `mail.submit` and `mail.submit.copy` `full` with or without `smtp`, the detail naming the channel.
- config.sample.toml: the SMTP block says Gmail and Graph send by themselves.

Not run live yet: a send through `messages.send` on the Google test account, a large message among them (`send` posts JSON, not the upload endpoint, as `import` does).

## Spec

- [sync](../spec/sync.md): *A send channel belongs to at most one source*, the send requirement (pick, transient verdicts, `Bcc`, build without a channel) and the unconfirmed send requirement name Gmail's `messages.send`.
