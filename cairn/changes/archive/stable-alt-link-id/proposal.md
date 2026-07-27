---
cairn: change
id: stable-alt-link-id
status: landed
created: 2026-08-01
---

# The link id must not depend on which tier computed it

## Why

A production posteo sync re-fetched the same 13 messages on every run (and
duplicated them). Investigating the store: exactly 13 items were stuck at `Meta`
level (level 1) while 8305 were `Full` (level 2), and each stuck item had a `Full`
twin bound to the *same server UID* under a **different link id**:

- Meta ghost: `alt:…|2019-04-16T11:40:14`**`+00:00`**`|…`
- Full item:  `alt:…|2019-04-16T11:40:14`**`Z`**`|…`

The `alt:` fallback link id (used when a message has no `Message-ID`) embeds the
date. The `Meta` link id is built from the IMAP ENVELOPE date via `chrono`, whose
`to_rfc3339()` writes UTC as `+00:00`; the `Full` link id is built from the body
via `mail_parser`, whose `to_rfc3339()` writes UTC as `Z`. Same instant, different
string — so a message links one way at `Meta` and another at `Full`: the `Meta`
item is stranded (never reaches `Full`, re-fetched forever) and its body lands
under a second, `Full` link id. A link id is an identity; computing it two
different ways breaks that identity.

## What

Make the `Meta`/ENVELOPE path format dates **exactly** like the `Full`/mail_parser
path — `chrono`'s `to_rfc3339_opts(SecondsFormat::Secs, true)`: UTC as `Z`, an
offset as `+hh:mm`, seconds precision. This is the format already stored for every
healthy item, so it changes no existing link id — it only stops new divergence. A
unit test asserts the `Meta` and `Full` link ids are byte-identical for a UTC and
an offset date (the `Z`/`+00:00` case and a `+02:00` case). The meta summary's
`date` is aligned the same way, so a `Full` upgrade no longer rewrites the row
just to reformat the date.

The 13 already-stranded ghosts do not self-heal (they are orphaned `+00:00`
items); they are removed by a one-time cleanup (delete level-1 items whose
`(collection, source, handle)` also binds a level-2 item — verified to remove
exactly the 13, cascade their bindings, and leave the 8305 healthy items intact).
