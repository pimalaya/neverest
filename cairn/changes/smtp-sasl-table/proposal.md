---
cairn: change
id: smtp-sasl-table
status: landed
created: 2026-08-27
---

# The send channel names a SASL mechanism

## Why

Every other credential in the configuration is a `sasl` table naming one of six
mechanisms. The send channel was the exception: a flat `smtp.login` and
`smtp.password`, hardwired to SASL LOGIN at the call site.

The shape was wrong twice over.

**It cannot express what the account already has.** Submission is the other half
of the same mail account, and a provider requiring an OAuth token for IMAP
requires one for submission too. LOGIN has nowhere to put a token, so the wizard
had to detect the case and apologise for it in prose, then prompt for a password
that the provider does not accept. Gmail and Microsoft accounts are exactly this
case, and they are two of the three the wizard discovers.

**It reads as a different kind of thing than it is.** `imap` and `smtp` sit side
by side under one account, configured within a line of each other, and a reader
comparing them finds `imap.sasl.plain.username` against `smtp.login` and has to
work out whether the asymmetry means something. It does not: io-smtp frames all
six mechanisms, LOGIN was simply the one wired up.

io-smtp already accepts the same `io_sasl::Sasl` io-imap does, and `SaslConfig`
already resolves into one. The flat fields were the only thing in between.

## What

- `SmtpConfig` carries `sasl: Option<SaslConfig>` in place of `login` and
  `password`, and orders its fields as `ImapConfig` does. Omitting the table is
  the unauthenticated relay, which is what `None` already meant.
- `server` accepts a bare authority, read as `smtps://`, resolved by the same
  match the IMAP one goes through.
- The wizard offers to reuse the IMAP table whatever mechanism it names, and
  asks whether the server authenticates at all before offering its own menu.
  Keyring entries are keyed per service, so an account's two do not collide.
- `io-smtp/scram` joins the `smtp` feature, so SCRAM-SHA-256 reaches the channel
  rather than falling through to `UnsupportedMechanism`.

## Not in scope

**No SMTP capability probe.** io-imap reads `CAPABILITY` into `SaslMechanism`
values, so the IMAP menu offers what the server confirmed; io-smtp exposes the
`AUTH` line as `&str` and no reader, so the SMTP menu offers what discovery
advertised. Mapping those strings belongs in io-smtp, beside the parser, not in
a wizard.

**No compatibility shim.** `smtp.login` was never released: 1.0.0-rc is the
first version to carry a send channel at all. `deny_unknown_fields` refuses the
old spelling by name rather than ignoring it, which is the outcome that matters,
since a silently dropped credential opens an unauthenticated session against a
server that requires one.
