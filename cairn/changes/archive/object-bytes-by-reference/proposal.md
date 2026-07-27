---
cairn: change
id: object-bytes-by-reference
status: landed
created: 2026-07-31
---

# Object bytes by reference (streaming remote)

## Why

The remote side of bounded-memory body transfer. `EmailRemote::fetch_full` calls
`get_message` → `Vec<u8>` and `append` uploads a `Vec<u8>`, so a 60 MB message is
a full-size allocation on fetch and again on append — peak memory `O(largest
message)`, an OOM risk on constrained devices. io-imap already exposes
`fetch_body_stream(sink: impl Write)` and `append_stream(source: impl Read, len)`;
this change wires neverest onto them.

Paired with io-replica's [`object-bytes-by-reference`] (the fetched-body /
`StoreObject` shape) and io-pimdir's `object-bytes-by-reference` (streaming blob
I/O).

## What

- `fetch_full`: stream via `io_imap::fetch_body_stream` straight into the pimdir
  blob store; sniff the header prefix (to the blank line) for the `Message-ID`
  link id and the summary; stream + hash the rest. Return the object by
  `(hash, size)` — no `Vec`.
- `append`: `io_imap::append_stream` from the blob `Read`, `len` = the stored
  object size (IMAP needs the octet count up front).
- m2dir backend: streamed get/add (file copy).
- `hash.rs`: incremental FNV so hashing folds into the copy.

## Scope / non-goals

- Depends on io-replica + io-pimdir `object-bytes-by-reference`.
- Bounds memory, not throughput; a single transfer is still download-then-upload.
  Concurrency is the separate `concurrent-size-ordered-fetch` change.
