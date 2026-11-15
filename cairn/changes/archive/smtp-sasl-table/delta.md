---
cairn: change
change: smtp-sasl-table
---

# Delta

## MODIFIED Requirements

### Requirement: The send channel authenticates like the sync side
A source's `smtp` table SHALL be spelled as its `imap` one: a `server` that is
either a bare authority, read as `smtps://<authority>`, or a full `smtp://` or
`smtps://` URL; the same `tls` block and `starttls` switch; an optional `alpn`
list; and an optional `sasl` table naming exactly one mechanism out of
ANONYMOUS, LOGIN, PLAIN, OAUTHBEARER, XOAUTH2 and SCRAM-SHA-256.

The mechanism SHALL resolve through the same conversion the IMAP side uses, the
GS2 host and port coming from the resolved submission URL. An omitted `sasl`
table SHALL open an unauthenticated session, stopping after `EHLO` and sending
no `AUTH`. The retired flat `login` and `password` keys SHALL be refused by
name, never ignored: a dropped credential would authenticate nothing against a
server that requires it.

A build declaring the `smtp` feature SHALL enable io-smtp's `scram` feature, so
a configured SCRAM-SHA-256 reaches the wire instead of being reported as an
unsupported mechanism.

### Requirement: The wizard configures one channel from the account's credentials
The wizard SHALL offer to back the send channel with the IMAP `sasl` table
whatever mechanism it names, a token mechanism included. Declining SHALL ask
whether the submission server authenticates at all before offering a mechanism
menu, so a relay taking no `AUTH` stays reachable. Credentials prompted for a
service SHALL be keyed under that service, so an account's IMAP and SMTP secrets
do not collide.

The SMTP menu SHALL be keyed on the capabilities discovery advertised rather
than on a live probe: io-imap reads `CAPABILITY` into mechanism values and
io-smtp offers no equivalent reader for the `AUTH` line.
