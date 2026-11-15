---
cairn: change
id: the-mail-spellings-are-refused
status: landed
created: 2026-11-15
---

# The mail spellings are refused

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, line N8; audit finding F38) found neverest still reading the spellings v0.2.0 kept "for one release" when the sync went kind-neutral: `mailbox` for `collection` and `message` for `item` in every backend table, `message` for the account's `item`, and `filters` for `collection.filter`. They were serde aliases, so a configuration carrying one loaded silently, and the account-level `mailbox` was refused under the name `collection`, which the person never wrote. Every other key an earlier neverest read is refused by name in the family sentence (pimalaya-config's `removed_key!`), pointing at MIGRATION.md.

## What

- A backend table's `mailbox` and `message` tables, the account's `mailbox` and `message` tables, and a backend table's `collection.filters` key are refused by name as the file is parsed, in the family sentence, each naming what to write instead.
- The account-level `mailbox` is refused under its own name, with the replacement the account-level `collection` names.
- CHANGELOG (breaking, under Removed, and the upgrade list) and MIGRATION.md (before and after, in the section of the coming release).

The keys refused: `mailbox` and `message` (tables) under the account; `<protocol>.mailbox` and `<protocol>.message` (tables) and `<protocol>.collection.filters` (key) under every backend table, written under the account or under `sources.<name>` or `targets.<name>`.

## Decisions

- **One type a key**, as the earlier refusals: the backend-level `mailbox` and `message` share one type each across every backend table (the `source_config!` macro), so their sentence names the key as it sits in the backend table (`mailbox`), the TOML error pointing at its line, and its replacement gives `imap` as the example.
- **No grace period.** The aliases were announced for one release in v0.2.0 and survived v0.3.0; the coming release is the major the alignment round prepares.
