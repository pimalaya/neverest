---
cairn: tasks
change: a-graph-session-stays-bounded
---

# Tasks

- [x] `src/msgraph/client/wire.rs`: watched stream, connector, failure classes; scripted fake server for tests.
- [x] `src/msgraph/client.rs`: reuse only after a whole answer, idle reopen at 60 s, one resend of a read on a reused connection found dead; never on a timeout or a write.
- [x] 404 convergence: flags, delete, move (checked by reading the message), contacts and events deletes; copy stays an error.
- [x] Sizes: `$expand` of the MAPI size on band listings, summary reads and the whole-folder delta (fallback on 400); `SizeHints` shared through `MsgraphAccount`; envelope size.
- [x] Body planning by stated octets (20 gets, 4 MiB, alone past 1 MiB or unsized), sizes read first when several are missing, each body committed as decoded, peak logged.
- [x] Per-get `Retry-After` in batches; a stated wait over two minutes gives the source up.
- [x] `src/msgraph/client/mail.rs`: delta and pass followed page by page into ids and marks; the row cache replaced by the band's marks, dropped by the pass.
- [x] Unit tests over the scripted server and the fake folder; clippy and tests on default, `--all-features` and MOA's feature set.
- [x] Spec folded, log written, CHANGELOG.
