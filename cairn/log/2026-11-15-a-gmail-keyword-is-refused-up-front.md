---
cairn: log
change: a-gmail-keyword-is-refused-up-front
date: 2026-11-15
---

# A Gmail keyword is refused up front

Line N28 of the CLI alignment round of 2026-11-15 (from himalaya's H32).

## What landed

- src/offline/capability.rs: a Gmail source declares `mail.flags.keywords` `none`, "Gmail drops every keyword but $Important, which mail.flags.important states", where it declared `partial`, "$Important only". `$Important` keeps its own `full` row and `$Junk` its `none` one.
- Tests: `a_gmail_source_refuses_any_keyword_but_important_at_the_gate` (src/offline/driver.rs) declares a Gmail store, then queues `$Custom`, refused naming `mail.flags.keywords`, and `$Important`, queued with no partial; `a_gmail_source_states_its_important_and_junk_rows` reads `none` and the detail. With the `partial` row put back, both fail: `$Custom` is queued.
- CHANGELOG: a **BREAKING** Changed entry and an upgrade line. MIGRATION.md: a before and after under the next release.

## Breaking

A `set-flags` changing `\Seen` or `\Flagged` with another keyword on a Gmail label is refused whole, where its `\Seen` or `\Flagged` part reached Gmail; an `add`, `copy` or `move` into a Gmail label of a message carrying a keyword is refused, where it landed without the keyword; such actions queued before park at the next drain. A keyword alone never reached Gmail.

## Capabilities moved

- [sync](../spec/sync.md): *Gmail is a native mail source* declares `mail.flags.keywords` `none` and gains the scenario *A keyword set on a Gmail message*.
