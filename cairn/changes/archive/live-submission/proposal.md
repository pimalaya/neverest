---
cairn: change
id: live-submission
status: landed
created: 2026-08-25
---

# The one path no server had ever answered

## Why

Every other side of this crate has met a real server: IMAP against two Stalwarts, CardDAV against a Radicale, the duplicate freeze and the relay against both. Submission had not. Its tests drive `send_one` against an in-process sink that accepts whatever the client says, so what they prove is that the envelope in the payload reaches the socket, and nothing about whether a server takes it.

It did not. Neverest greets with `EHLO localhost`, and `localhost` is not a domain name a server is obliged to believe: RFC 5321 §4.1.4 entitles it to check, and Stalwart does, answering `550 5.5.0 Invalid EHLO domain`. Every submission through an `<side>.smtp` channel failed at the greeting, before `MAIL FROM`, against any server that checks. The failure is transient by disposition, so the intent stayed pending and the run reported nothing beyond a warning: the queue silently filled and no mail left. Himalaya and himalaya-tui already greet with the loopback address literal, which is the form RFC 5321 §4.1.3 reserves for a client with no resolvable name of its own; only this crate did not.

Neither the sink nor a live IMAP test could have caught it, which is the point: the harness had two Stalwarts running and reached neither one's SMTP port.

## What

- `ehlo_domain` sends `[127.0.0.1]`, the address literal, matching what the other clients in this org send.
- `tests/stalwart2.sh` publishes each instance's port 25 (A on 2525, B on 2526), so the harness that already runs the servers exposes the channel too.
- `tests/submit.rs`, the live run: a body staged in the blob tree, an intent enqueued through `PimdirProducer` exactly as a frontend produces one, then the sync sends it, acknowledges it, and the next run pulls the delivered message back into the store. Submission and replication proven as one chain rather than two.

The test's marker is minted per run, since the server keeps what earlier runs delivered and a constant marker would let one of those pass a run that sent nothing. It accepts delivery to `Junk Mail` beside `INBOX`: port 25 offers no `AUTH`, so the message arrives unauthenticated with a sender in the server's own domain and the filter reads that as spoofing. Where the server files it is the server's call; that it arrives, and that the sync collects it wherever it is, is this chain's.
