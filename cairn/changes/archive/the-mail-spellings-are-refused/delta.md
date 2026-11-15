---
cairn: delta
change: the-mail-spellings-are-refused
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: The configuration refuses every key it does not read
The configuration SHALL refuse at load, naming it, every key it does not read, at every level: the document root, which holds `accounts` alone; an account; a `sources.<name>` or `targets.<name>` table; a backend table, written under a source or directly under the account; and every table nested in one (`tls`, `sasl`, `auth`, `collection`, `item`, `flag`, `smtp`, `store`, `conflict`). A source table naming no backend table, or more than one, SHALL be refused at load, naming the backends it found.

A key an earlier neverest read and removed SHALL be refused by name in the family sentence, pimalaya-config's `removed_key!`: "The `x` key is removed, <why>: <what to write instead>, see MIGRATION.md", `table` in place of `key` for a table, as the file is parsed, so in an account no command opens as in one it does: `left` and `right`, `folder`, `envelope`, the account-level `collection`, `mailbox` and `message`, `collection.namespace` on any source (one written directly under the account included), the `mailbox` and `message` tables and the `collection.filters` key of any backend table, `store.retention` and `store.hydration`, and the flat `login` and `password` of an `smtp` table. No key SHALL be read under an old spelling: an alias either loads a configuration the person thinks is refused or refuses it under a name the person never wrote.

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
- THEN it is refused with "The `folder` table is removed, …: write its filter as `<protocol>.collection.filter`, …, see MIGRATION.md"

#### Scenario: A v0.2 mail spelling
- GIVEN an account carrying `imap.mailbox.create = false`
- WHEN the configuration is loaded
- THEN it is refused with "The `mailbox` table is removed, …: write `collection` in its place, as in `imap.collection.filter`, see MIGRATION.md", where it loaded as `imap.collection.create`

## REMOVED Requirements

None.
