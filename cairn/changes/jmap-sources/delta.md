---
cairn: delta
change: jmap-sources
---

Written for the mail and contacts sources. The calendar source adds its own requirement when it opens.

## ADDED Requirements

### Requirement: JMAP is a native mail source
A `jmap.mail` domain, the source `jmap`, SHALL open protocol-direct over io-jmap and sync `message/rfc822`. Its collections SHALL be the account's mailboxes, keyed and named by their name path (`Parent/Child`), so a JMAP endpoint and an IMAP endpoint on one account pair their folders, each with the role its `role` states.

A handle SHALL be the `Email` id, stable across mailboxes. A full round SHALL be `Email/query` over the mailbox, a page at a time, and SHALL carry as its checkpoint the `Email` state read before its first page. A delta SHALL be `Email/changes` since that state, read once a run for the account and projected on each mailbox: a changed email the mailbox holds is listed, one the store binds there that left it is stated vanished, a destroyed one is stated vanished wherever it is bound. `cannotCalculateChanges` SHALL restart a full round. A scoped round SHALL narrow by `after` with a margin, state `narrowed` on every page, and hand every id of the mailbox on its last page.

A message SHALL be named off its raw header fields (`headers`), never off the properties the server parsed, and its body SHALL be the octets of its blob.

Keywords and flags SHALL map one to one. An add SHALL go through a blob upload and `Email/import`, a move SHALL be one patch dropping one mailbox and adding the other, and a delete SHALL drop the mailbox, destroying the email when it was its last. A body update SHALL be refused. A write on an email gone SHALL count as done.

Auth SHALL be what the `auth` table names (`header`, `bearer` or `basic`), resolved once per run with every other credential. The session SHALL be read from the configured URL and no redirect followed.

#### Scenario: An email filed in two mailboxes is one item
- GIVEN an email whose `mailboxIds` names `INBOX` and `Work`
- WHEN the account syncs
- THEN the store holds one item filed in both collections and its body is downloaded once

#### Scenario: An email leaves a mailbox
- GIVEN an email bound in `INBOX` and `Work`, removed from `Work` on the server
- WHEN the next run reads `Email/changes`
- THEN it is stated vanished in `Work` and stays bound in `INBOX`

#### Scenario: A state the server forgot
- GIVEN a checkpoint the server answers `cannotCalculateChanges` to
- WHEN the mailbox is enumerated
- THEN a full round opens and nothing is deleted on the answer alone

### Requirement: A JMAP mail source sends by itself
A `submit` intent on a `jmap` source SHALL be an `EmailSubmission` created through the source's own session, the message imported as a draft first and filed as sent on success, under `identity-id` where the source names one, else under the identity matching the message's `From`. The source SHALL need no `smtp` channel, and SHALL be refused one.

### Requirement: A JMAP contacts source syncs the address books
A `jmap.contact` domain, the source `jmap-contact`, SHALL sync `text/vcard` over the session's contacts capability (RFC 9610), through the session the `jmap` table holds. Every `AddressBook` SHALL be a collection keyed by its id and named by its `name`, a card in several books being one item filed in each. The body SHALL be the vCard io-jmap converts the `ContactCard` to and from (RFC 9555), the projection living in the protocol crate, and the item's identity the card's `uid`.

The revision of a card SHALL be a hash of its JSON, JMAP stating none. A write SHALL carry `ifInState`, so a card edited on the server since the run's listing is refused and merged by the next run, never overwritten.

#### Scenario: A card edited on both sides
- GIVEN a card edited in the store and on the server since the last sync
- WHEN the run pushes the store's edit
- THEN the write is refused on `stateMismatch`, nothing is overwritten, and the next run merges the two

## MODIFIED Requirements

### Requirement: Sources are remote backends only
JMAP joins IMAP, Microsoft Graph and Gmail for `message/rfc822`, and CardDAV, Microsoft Graph and Google People for `text/vcard`. The parenthetical "JMAP as its backend lands" is kept for `text/calendar` alone.

### Requirement: A backend under the account is a source named after its protocol
The `jmap` table SHALL be sugar for one source a domain it names, as `msgraph` is: `jmap-mail` for `jmap.mail`, `jmap-contact` for `jmap.contact` (`jmap-calendar` for `jmap.calendar` when it opens), each holding the table's session (`server`, `tls`, `alpn`, `auth`) and that domain's table alone, a domain written `= true` taking its defaults. Under `sources.<name>`, a `jmap` table SHALL name one domain, the source keeping `<name>`. `collection`, `item`, `flag` and `pool-size` written directly in `jmap`, and a `jmap` table naming no domain, SHALL be refused by name, pointing at `jmap.mail`.

### Requirement: Every remote backend is a cargo feature
`jmap` gates the JMAP backends. A JMAP source in a build without it fails when opened, coded `backend-not-built` and naming `jmap`, as every other backend does; "a JMAP source, which no feature opens yet, fails saying so" is removed.

### Requirement: A send channel belongs to at most one source
A JMAP mail source sends by itself, beside Microsoft Graph mail.

### Requirement: A collection is keyed by its backend id and named by its display name
The mailbox name path keys a JMAP mail collection, the `AddressBook` id a JMAP contacts one.

### Requirement: Every source declares its capabilities before the drain
A JMAP source declares the capabilities of its kind like every other; "A JMAP source stays undeclared" is removed.

## REMOVED Requirements
