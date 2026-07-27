---
cairn: log
change: object-bytes-by-reference
landed: 2026-07-31
---

# Object bytes by reference (streaming remote)

Wired the remote onto io-imap's streaming primitives so a message body never
sits in memory whole. `hash.rs` gained an incremental `Hasher` (fed chunk by
chunk; a test proves the chunked digest equals the one-shot one, so a streamed
body still dedups). The `Client` seam replaced buffered `get_message`/
`add_message` with `get_message_stream(sink)` and
`add_message_stream(source, len, message_id)`. The IMAP backend implements them
over `io_imap::fetch_body_stream` / `append_stream` (the Message-ID for the
no-UIDPLUS UID recovery now rides the link id, since the body streams past
unparsed). `EmailRemote::fetch_full` streams the body straight into the pimdir
blob store through a `HydrateSink` that tees each chunk into the blob writer, the
hash, and a header-prefix capture (bounded by a cap) — returning the object as
`Persisted { hash, size }`, no `Vec`. `append` streams from the blob file with
its byte length. The m2dir backend gets streaming wrappers (buffered internally —
local disk; a native file stream is a follow-up).

Verified end-to-end: the Stalwart integration roundtrip (m2dir A → IMAP → m2dir B)
now carries a ~3 MB body and passes, exercising streamed fetch → blob → append
over a real server. Unit tests and fmt clean.

Depends on io-replica + io-pimdir `object-bytes-by-reference`.

Spec updated: `sync` (ADDED: bodies transfer with bounded memory).
