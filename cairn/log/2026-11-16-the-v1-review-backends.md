---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 3: backend duplication

- One test fake, src/scripted.rs (per-connection `Step` scripts, an optional responder), replaces msgraph's `wire::fake`, gcal's `Recorded`/`Answering`, gpeople's `Canned`/`Scripted`, dav's `Replay` and gmail's `scripted::Wire`; test counts unchanged by the move.
- Fixed: gcal and gpeople kept a stream that failed mid-exchange and wrote the next request to it. `a_stream_that_failed_mid_exchange_is_reopened` (gcal and gpeople) failed on the old code with "HTTP/1.1 send failed: reached unexpected EOF" and passes now.
- The Google request loop is one generic `google_send` in src/throttle.rs (`GoogleSession`, `GoogleError`), carrying gmail's rule that an I/O error marks the stream dead.
- TLS stream opening is one `open_tls` in src/http.rs for gcal, gpeople, gmail and msgraph; gpeople's and gmail's first connection still go through their io- crates' `connect`, which open the same stream.
- Capability moved: sync (a Google stream that failed is reopened).
