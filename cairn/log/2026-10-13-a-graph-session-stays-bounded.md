---
cairn: log
change: a-graph-session-stays-bounded
date: 2026-10-13
---

# A Graph session survives idle connections and stays bounded

What the Android client learned on Microsoft 365, brought to the Graph source.

## What landed

- src/msgraph/client/wire.rs: the watched stream (what an exchange wrote, whether an answer began, whether the stream ended), the connector, the failure classes, and a scripted Graph server for the tests.
- src/msgraph/client.rs: a connection is reused only after a whole answer the server kept open, reopened past 60 s idle; a read (a `GET`, a `$batch` of gets) found dead on a reused connection before any octet of its answer goes again once on a new one, never a timed-out read nor a write. Flags, deletes and moves on a message gone count as done (a move checked by reading the message), a copy stays an error. Bodies are planned by the MAPI size the listings state (`SizeHints`, shared by the source's connections through `MsgraphAccount`): batches of 20 gets and 4 MiB at most, a message over 1 MiB or of no stated size read alone, missing sizes read first when several lack one; each body is committed as soon as it is decoded, and each batch logs the octets it stated, held and decoded. Each throttled get of a batch waits its own `Retry-After`; a stated wait over two minutes gives the source up at once. The envelope carries the MAPI size; the `Meta` tier reads summaries by id.
- src/msgraph/client/mail.rs: the band listing, the summary reads and the whole-folder delta ask for the size `$expand` (the delta without it once refused); a delta and the pass are read page by page into ids and a few octets; the row cache is replaced by the flags of what a band round listed, dropped by its pass, which lists again only what moved since.
- src/msgraph/client/contacts.rs, calendar.rs: a delete of a contact or event gone counts as done.
- Tests: 22 new unit tests (the connection over the scripted server, 404 convergence, batch planning and the octets held, sizes read first, per-get `Retry-After`, the delta `$expand` fallback, the pass relisting only what moved). The whole suite, and clippy with the default features, `--all-features` and MOA's set (`--no-default-features --features rustls-ring,imap,smtp,dav,msgraph,gpeople,gcal,gmail`).

## Capabilities moved

- sync: added "A Graph connection is reused only after a whole answer", "A Graph write on an item gone counts as done", "A Graph listing keeps a few octets a message"; modified "Graph mail bodies are fetched in JSON batches", "A Graph mail folder keeps one delta link over the whole folder" (the pass), "Microsoft Graph is a first-class source" (the `Meta` tier), "Every listed member arrives named" (the size `$expand`).
