---
cairn: delta
change: side-owned-send-channel
---

# Delta

## MODIFIED Requirements

### Requirement: The Outbox is local-only and flushes through the send channel
The `Outbox` collection SHALL never be enumerated against a remote. After the
queue drain, every placement staged as a creation in it SHALL be sent through a
side's send channel and dropped from the store on success. The channel belongs
to the side, not to the account: a side either sends natively (the Graph
`sendMail` action, which saves to Sent itself) or carries its own `<side>.smtp`
table (`server` `smtps://…:465` or `smtp://…:587` + `starttls`, optional
login/password), beside the backend block it completes. Sides SHALL be walked
in configuration order (`left`, then `right`) and the first one offering a
channel SHALL flush the queue.

The SMTP envelope comes from the placement's outbox meta (`v`, `from`,
`rcpts`, …); a per-message failure leaves the placement queued for the next run
and never kills the run. Message content is never logged. The queue itself is
unconditional; only the channels draining it are gated (`smtp`, `msgraph`), so a
build with neither accumulates queued sends and warns instead of flushing, and a
channel the running build cannot open warns and leaves the queue put.

## ADDED Requirements

### Requirement: A side pairs one backend with its send channel
A side SHALL be a table naming exactly one backend (`<side>.imap`,
`<side>.jmap`, `<side>.gmail`, `<side>.msgraph`) and, optionally, the
`<side>.smtp` channel completing it. A backend key that matches no backend
SHALL be refused. The account root SHALL carry no `smtp` table: a configuration
keeping one SHALL fail to parse rather than silently stop sending.

## REMOVED Requirements

None.
