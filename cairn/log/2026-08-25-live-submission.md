---
cairn: log
date: 2026-08-25
change: live-submission
---

# Submission met a server, and lost

The submission path had never been run against a real SMTP server: its tests
drive `send_one` against an in-process sink, and the two-Stalwart harness that
already ran for the relay and duplicate tests published only IMAP. Pointing the
first live run at one of those servers failed at the greeting.

Neverest greeted with `EHLO localhost`. Stalwart checks the argument, as RFC 5321
§4.1.4 lets it, and answers `550 5.5.0 Invalid EHLO domain`; the session never
reached `MAIL FROM`. The disposition is transient, so the intent stayed pending
and the run reported only a warning: against any checking server, the queue
filled and no mail left. `ehlo_domain` now sends the loopback address literal
`[127.0.0.1]`, RFC 5321 §4.1.3's form for a client with no resolvable name,
which is what himalaya and himalaya-tui already send.

`tests/stalwart2.sh` publishes port 25 for both instances (A on 2525, B on 2526),
and `tests/submit.rs` is the live run behind it: a body staged in the blob tree,
a `submit` intent enqueued through `PimdirProducer` the way a frontend produces
one, the sync sending and acknowledging it, and the next sync pulling the
delivered message back into the store. The marker is minted per run, so a message
an earlier run left on the server cannot pass a run that sent nothing, and
delivery to `Junk Mail` counts beside `INBOX`: port 25 offers no `AUTH`, so the
message arrives unauthenticated with a sender in the server's own domain and the
filter reads that as spoofing. Where it is filed is the server's call; that it
arrives and that the sync collects it is not.

Capability moved: **sync** (a submission greets with an address literal).
