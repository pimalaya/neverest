---
cairn: change
id: duplicate-link-id-mints-an-item
status: landed
created: 2026-08-28
---

# Two resources under one UID sync as two items

> Cross-repo change, same id in eight repositories, in this order:
> **pimdir** (the rule) → **io-replica** (the mint) → **io-pimdir** (the column goes) → **io-webdav** (the refusal is named) → **neverest** (here) → **himalaya**, **cardamum**, **calendula**.
>
> This **supersedes `duplicate-link-id-freeze`** (landed 2026-08-25). Waits on the io-replica and io-webdav releases.

## Why

Reported by the user on a Posteo CalDAV account, 2026-08-28: every sync of `caldav/default` printed the same four `fetch item` lines, run after run, naming resources the store never came to hold.

What was happening, end to end. Four `UID`s are held by the server under two hrefs each (`<uid>@google.com.ics` and `<uid>%40google.com.ics`, both written by Thunderbird, three pairs differing only in `DTSTAMP` and one being two genuinely different meetings). The engine binds one href per identity and freezes the second. The frozen twin never gets a row, so `itemize_fetches` reads it as an unfetched body and reports it. Posteo advertises no `sync-collection` for that collection, so the fallback listing enumerates in full every run, and the twin is rediscovered, downloaded whole (a calendar resource resolves its identity only from its body), refrozen and left unreferenced: four bodies and four orphan blobs per run, and a report line naming work that could never complete.

The engine now mints a key for the second copy instead of freezing it, so the store holds both and the phantom line disappears on its own. What is left for this crate is the write side, where minting creates three ways to lose data quietly.

## What

- **A new resource is never named after a name already taken.** `resource_id` falls back to re-deriving the id from the body when the link hint is prefixed, and a duplicate's body carries a perfectly usable `UID`, so both copies of a pushed pair would be named `<uid>.ics`: the second `PUT` would overwrite the first on the target rather than be refused by it. The minted key's own suffix SHALL reach the resource name.
- **A create whose assigned handle is already bound is a rejection, not a binding.** A server treating the `UID` as its key may answer the `PUT` by updating the existing resource and returning its href. Nothing today stops that href being bound twice, and on a two-way run the next enumeration reads the missing second handle as vanished and propagates a delete. The push result SHALL be checked against the handles this source already binds in that collection.
- **A refused duplicate says why.** With io-webdav naming the `no-uid-conflict` refusal, the rejected push SHALL report it as such, naming the source, the `UID` and the collection, rather than a bare `409`. The line repeats every run, which is correct: it is an unresolved state with an action attached, and the run wrote nothing.
- **The ambiguity reporting goes**, with the state it reported: no ambiguity section, no `ambiguous` list in the text or `--json` report, no ambiguity itemisation in the drivers.

## Scope / non-goals

- **No repair verb.** Neverest does not delete a duplicate, does not re-`UID` one, and does not offer to. Which copy to keep is the user's judgement against their server, and the report gives them what they need to make it.
- **No warning for a duplicate that syncs cleanly.** A collection holding two resources under one `UID` mirrors as two items, silently. A report line is for a run that failed to write something, not for data the user already has.
- **The pull plan is not re-engineered.** `itemize_fetches` keeps reading the side rather than the projection; what changes is that the twin now leaves it by getting a row.
