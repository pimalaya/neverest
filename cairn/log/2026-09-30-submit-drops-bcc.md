---
cairn: log
change: submit-drops-bcc
landed: 2026-09-30
---

# A queued send no longer shows its Bcc recipients

The SMTP channel sent a queued body byte for byte, so a `Bcc:` field a producer staged reached every recipient. It was io-smtp 0.3 behaviour, fixed upstream in 0.4 (`SmtpMessageSendOptions::keep_bcc`, off by default); neverest now depends on io-smtp 0.5 and sends with the default options. The recipients still come from the payload's `rcpts`, so a Bcc address keeps its `RCPT TO`.

The 0.5 connect takes one `SmtpClientStdConnectOptions`; the channel passes `Proxy::None`, keeping the direct connection it made before.

No producer enqueues a `submit` intent yet, so no released path leaked; the test `a_bcc_recipient_is_in_the_envelope_but_not_in_the_transmitted_body` pins it before one does.

Capability moved: **sync**, the queued-submission requirement.
