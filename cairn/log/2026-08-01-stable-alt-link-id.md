---
cairn: log
change: stable-alt-link-id
landed: 2026-08-01
---

# The link id must not depend on which tier computed it

A production posteo sync re-fetched the same 13 messages every run (and
duplicated them). The store held exactly 13 items stuck at `Meta` (level 1)
against 8305 at `Full` (level 2); each stuck item had a `Full` twin bound to the
same server UID under a link id differing only in the date's UTC spelling:
`…T11:40:14+00:00…` (Meta) vs `…T11:40:14Z…` (Full).

Root cause: the `alt:` fallback link id (no `Message-ID`) embeds the date. The
`Meta` link id is built from the IMAP ENVELOPE via `chrono` (`to_rfc3339()` →
`+00:00` for UTC); the `Full` link id is built from the body via `mail_parser`
(`to_rfc3339()` → `Z`). Same instant, different string → the message linked one
way at `Meta`, another at `Full`, stranding the `Meta` item forever.

Fix: `envelope_date` formats the ENVELOPE date exactly like `mail_parser` —
`chrono`'s `to_rfc3339_opts(SecondsFormat::Secs, true)` (UTC `Z`, offset `+hh:mm`,
seconds). This is the format already stored for every healthy item, so no existing
link id changes; it only stops new divergence. Used by `envelope_link_id` and
`envelope_meta` (the meta alignment also spares a redundant row rewrite on the
`Full` upgrade). A unit test asserts the `Meta` and `Full` link ids are
byte-identical for a UTC date (the regression) and a `+02:00` offset date.

The 13 already-stranded ghosts do not self-heal. A one-time cleanup — delete
level-1 items whose `(collection, source, handle)` also binds a level-2 item —
was verified on a copy of the production db to remove exactly the 13, cascade
their bindings, and leave the 8305 healthy items intact. The production db was not
modified; the cleanup command is handed to the operator to run once (after
deploying this fix, before the next sync).

Spec updated: `sync` (MODIFIED "Bodies are content-addressed and deduped": the
link id MUST be computed identically across tiers, date formatted the one
canonical way).
