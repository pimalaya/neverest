---
cairn: log
change: tls-client-certificate
date: 2026-11-16
---

# A connection presents a client certificate

`tls.client.cert` and `tls.client.key` are read under every `tls` table and passed to pimalaya-stream's `TlsClient`, as himalaya does; the sample documents them under the shared network keys. Trivial change, no proposal. Capability moved: sync (a connection presents a client certificate when configured).
