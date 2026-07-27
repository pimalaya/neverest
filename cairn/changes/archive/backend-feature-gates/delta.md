---
cairn: change
change: backend-feature-gates
---

# Delta

## ADDED Requirements

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend,
`msgraph` for the Microsoft Graph backend, `smtp` for the SMTP submission
channel. All three SHALL ship in the default feature set. A missing backend
SHALL surface at runtime, never at build time: every feature combination
compiles, the configuration surface stays whole (every side config still
parses), and an unavailable backend fails when the side is *opened*, as the
JMAP and Gmail sides already do. A build with neither `smtp` nor `msgraph` has
no send channel and SHALL warn rather than flush. Each
optional backend crate SHALL take its TLS provider from neverest's own
`native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than
pinning one.

### Requirement: A backend owns its ALPN default
The `alpn` field of a side or channel config that has a backend crate SHALL be
optional, and unset SHALL mean that crate's own default (io-imap's `["imap"]`,
io-smtp's `["smtp"]`), resolved where the connection is opened. An explicit `[]`
SHALL skip ALPN. Neverest SHALL NOT restate a backend's default, in the config
schema or in the values the wizard writes, so the default lives in exactly one
place.

## MODIFIED Requirements

### Requirement: The Outbox is local-only and flushes through the send channel
The `Outbox` collection SHALL never be enumerated against a remote. After the
queue drain, every placement staged as a creation in it SHALL be sent through
the account's send channel — SMTP submission when the account configures an
`smtp` table (`server` `smtps://…:465` or `smtp://…:587` + `starttls`, optional
login/password), the Graph `sendMail` action for a Graph-backed account (which
saves to Sent itself) — and dropped from the store on success. The SMTP
envelope comes from the placement's outbox meta (`v`, `from`, `rcpts`, …); a
per-message failure leaves the placement queued for the next run and never
kills the run. Message content is never logged. The queue itself is
unconditional; only the channels draining it are gated (`smtp`, `msgraph`), so a
build with neither accumulates queued sends and warns instead of flushing, and a
configured `smtp` table in a build without the feature warns and falls back to a
Graph side if the account has one.

## REMOVED Requirements
