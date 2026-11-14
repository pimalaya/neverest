---
cairn: delta
change: an-unknown-key-is-refused
---

# Delta

## ADDED Requirements

### Requirement: The configuration refuses every key it does not read
The configuration SHALL refuse at load, naming it, every key it does not read, at every level: the document root, which holds `accounts` alone; an account; a `sources.<name>` or `targets.<name>` table; a backend table, written under a source or directly under the account; and every table nested in one (`tls`, `sasl`, `auth`, `collection`, `item`, `flag`, `smtp`, `store`, `conflict`). A source table naming no backend table, or more than one, SHALL be refused at load, naming the backends it found.

A key an earlier neverest read and removed SHALL be refused by name with its replacement, ending on the address of MIGRATION.md, by every command that opens the account and before anything connects: `left` and `right`, `folder`, `envelope`, the account-level `collection`, `collection.namespace` on any source (one written directly under the account included), `store.retention` and `store.hydration`, and the flat `login` and `password` of an `smtp` table.

#### Scenario: A typo beside a source's backend
- GIVEN `sources.work.imap.server = "…"` and `sources.work.imap-typo = 1`
- WHEN any command loads the configuration
- THEN it fails naming the unknown field `imap-typo`, where the key used to be ignored

#### Scenario: Two backends in one source
- GIVEN a source naming an `imap` and a `jmap` table
- WHEN the configuration is loaded
- THEN it fails naming `imap` and `jmap`, where the second used to be dropped

#### Scenario: A v0.1 table
- GIVEN an account carrying `folder.filter = "all"`
- WHEN a command opens it
- THEN it is refused naming `folder`, its replacement `<protocol>.collection.filter`, and MIGRATION.md

## MODIFIED Requirements

### Requirement: The send channel authenticates like the sync side
A source's `smtp` table SHALL be spelled as its `imap` one: a `server` that is either a bare authority, read as `smtps://<authority>`, or a full `smtp://` or `smtps://` URL; the same `tls` block and `starttls` switch; an optional `alpn` list; and an optional `sasl` table naming exactly one mechanism out of ANONYMOUS, LOGIN, PLAIN, OAUTHBEARER, XOAUTH2 and SCRAM-SHA-256.

The mechanism SHALL resolve through the same conversion the IMAP side uses, the GS2 host and port coming from the resolved submission URL. An omitted `sasl` table SHALL open an unauthenticated session, stopping after `EHLO` and sending no `AUTH`. The retired flat `login` and `password` keys SHALL be refused by name, never ignored, naming the `sasl` table that replaces them ("The configuration refuses every key it does not read"): a dropped credential would authenticate nothing against a server that requires it.

A build declaring the `smtp` feature SHALL enable io-smtp's `scram` feature, so a configured SCRAM-SHA-256 reaches the wire instead of being reported as an unsupported mechanism.
