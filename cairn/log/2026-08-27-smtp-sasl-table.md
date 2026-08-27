---
cairn: log
change: smtp-sasl-table
date: 2026-08-27
---

# The send channel names a SASL mechanism

`SmtpConfig` carried a flat `login` and `password` and `connect_smtp` hardwired
them to SASL LOGIN, the one exception to a configuration where every other
credential is a `sasl` table. It now carries `sasl: Option<SaslConfig>` and
orders its fields as `ImapConfig` does, so the two halves of a mail account read
the same. `server` accepts a bare authority through the same match the IMAP one
goes through, taking `smtps://`.

What the old shape could not express is the point: a provider requiring an OAuth
token for IMAP requires one for submission, and LOGIN has nowhere to put a
token. The wizard used to detect that, print an apology and prompt for a
password the provider does not accept; it now offers to reuse the IMAP table
whatever mechanism it names. Declining asks whether the server authenticates at
all before offering a menu, which is how the unauthenticated relay stays
reachable now that a blank login no longer means it. Prompted secrets are keyed
per service rather than always under `-imap`.

`io-smtp/scram` joins the `smtp` feature. io-sasl's `scram` was already on, so
`Sasl::ScramSha256` existed in the build while io-smtp's arm for it was compiled
out: a configured SCRAM-SHA-256 would have reached the wire as
`UnsupportedMechanism`.

The old spelling is refused by `deny_unknown_fields` rather than ignored, which
is the outcome that matters: a silently dropped credential opens an
unauthenticated session against a server that requires one. Nothing needs
migrating, `smtp.login` having never been released.

No SMTP capability probe. io-imap reads `CAPABILITY` into `SaslMechanism`
values; io-smtp exposes the `AUTH` line as strings and no reader, so the SMTP
menu offers what discovery advertised. Mapping those strings belongs in io-smtp,
beside the parser.

Capabilities moved: **sync** (the send channel's schema and the wizard that
writes it).
