---
cairn: tasks
change: object-bytes-by-reference
---

# Tasks

- [ ] `EmailRemote::fetch_full`: stream via `io_imap::fetch_body_stream` into the
      pimdir blob store; header-sniff the prefix for `Message-ID` + summary;
      stream + hash the rest; return a persisted `(hash, size)` body.
- [ ] `EmailRemote::append`: `io_imap::append_stream` from the blob `Read`, with
      `len` = the stored object size.
- [ ] m2dir backend: streamed `get_message` / `add_message` (file copy).
- [ ] `hash.rs`: incremental FNV fed in chunks.
- [ ] Delete the `Vec<u8>` get/add paths.
- [ ] Tests (extend the Stalwart roundtrip with a multi-MB message; assert
      bounded memory or at least a streamed path).
- [ ] Fold spec: `sync`. Log entry.
