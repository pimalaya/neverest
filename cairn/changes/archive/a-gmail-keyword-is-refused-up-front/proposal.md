---
cairn: change
id: a-gmail-keyword-is-refused-up-front
status: landed
created: 2026-11-15
---

# A Gmail keyword is refused up front

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, line N28, from himalaya's H32) found a Gmail source declaring `mail.flags.keywords` `partial` ("$Important only, which mail.flags.important states too"). The store's gate lets a `partial` capability through with a warning, so a frontend queued any keyword on a Gmail message, himalaya's receipt warning since 85a12d9a, and the push dropped it: src/gmail/client.rs `label_patch` maps `\Seen`, `\Flagged` and `$Important` to `UNREAD`, `STARRED` and `IMPORTANT` and drops every other flag, a keyword turned label filing the message in a new collection.

`$Important` does not need that row: a Gmail source declares `mail.flags.important` `full`, and io-pimdir's gate (`capability::gating`) reads `$Important` and `$Junk` under their own rows where a source states them, `mail.flags.keywords` gating every other keyword. The `partial` was kept "for a reader predating those rows", but every io-pimdir since 0.9 reads them, and a reader old enough not to cannot open a store of this version (storage-draft-11, io-pimdir 0.12 on): no reader is left for it.

## What

- A Gmail source declares `mail.flags.keywords` `none`, detail "Gmail drops every keyword but $Important, which mail.flags.important states". The store's gate then refuses, in every frontend, an action changing a keyword other than `$Important` on a Gmail label: a `set-flags`, or an `add`, `copy` or `move` landing a message that carries one. `$Important`, `\Seen` and `\Flagged` are unchanged.
- The owner's drain backstop parks such an action queued before the upgrade, rather than applying it with the keyword dropped.

## Breaking

Yes, for what passed before with a warning:

- a `set-flags` changing `\Seen` or `\Flagged` together with another keyword is refused whole, where its `\Seen` or `\Flagged` part reached Gmail and the keyword was dropped;
- an `add`, `copy` or `move` into a Gmail label of a message carrying a keyword (a Thunderbird `$Forwarded` or `NonJunk` on a message from an IMAP folder, say) is refused, where the message landed without it, as one carrying `\Answered` already was;
- such an action queued before the upgrade parks at the next drain.

A keyword alone never reached Gmail, so nothing that synced is lost.

## Not in scope

- Letting an `add`, `copy` or `move` land with the keywords its target cannot hold stripped rather than refused: the gate is io-pimdir's.
- `pimdir tag` still only logging the partial support: io-pimdir's tag staging (task line N28).
