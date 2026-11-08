---
cairn: delta
change: a-sent-copy-takes-the-providers-flags
---

## MODIFIED Requirements

### Requirement: A sent copy is filed once sent
A `submit` asking for `copy` SHALL file the message in that collection only after the send succeeds: natively where the provider files sent mail itself (the Gmail and Graph backends, and an IMAP source whose IMAP and SMTP hosts are a pair neverest knows to file what the SMTP server sends in that mailbox's Sent), otherwise by replacing the intent with an `add` of the message, `\Seen` set, drained in the same run (pimdir STORAGE §15.5, Annex B.2).

A copy staged where the server files its own lands on that copy (pimdir SYNC §6) and pushes its staged `\Seen` onto it, which marks a message sent to oneself read where the provider keeps one message for both mailboxes; the provider's copy, listed by the next round, is the one the store binds.

#### Scenario: A message sent to oneself through a filing pair
- **GIVEN** an IMAP source whose SMTP server files what it sends, and a queued `submit` to the account's own address with `copy` set to its Sent
- **WHEN** the account syncs until the message is delivered
- **THEN** the inbox copy is unread, Sent holds the message once, and the store binds that one copy

#### Scenario: A server that files no copy
- **GIVEN** an IMAP account whose SMTP server files nothing in Sent
- **WHEN** a `submit` with `copy` set to Sent is performed and the account syncs
- **THEN** neverest uploads the message to Sent once, seen
