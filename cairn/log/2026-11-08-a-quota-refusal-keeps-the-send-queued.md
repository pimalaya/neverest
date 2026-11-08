---
cairn: log
change: a-quota-refusal-keeps-the-send-queued
date: 2026-11-08
---

# A quota refusal keeps the send queued

Every SMTP 5xx parked its send for good, Gmail's daily sending limit (`550 5.4.5 Daily user sending limit exceeded`) among them, so a message that only had to wait for the next day waited for a person instead. Found by reading the code against the Android fix of 2026-11-06, not seen live. The regression test failed on 4edcb06: both queued sends parked, the second attempted and refused too.

## What landed

- `smtp_disposition` (src/offline/submit.rs) reads the reply text beside the code (`reply_of`, from io-smtp 0.6's `Rejected { code, message }`). A 5xx refusing for a sending limit is `Disposition::Limited`: transient, the row pending, and the channel `held`, so the run's other sends are reported not attempted. The SMTP session still quits cleanly, unlike a lost one.
- The set, from the providers' pages (2026-11-08): codes `5.4.5` (Gmail's daily user and relay limits; RFC 3463 congestion), `5.1.90` (Exchange Online daily recipient limit), `5.7.232`, `5.7.233`, `5.7.236` (Exchange Online tenant external limits); wording `SubmissionQuotaExceeded` (Exchange `554 5.2.0`), `submission quota` (Exchange `5.2.2`), `sending limit exceeded` and `rate limited` (Gmail relay `5.7.1`), `daily limit for message recipients`. Exchange's own full mailbox (`5.2.2 ... QuotaExceededException`) and blocked senders (`5.1.8`, `5.7.501` to `5.7.503`, `5.7.70x`) stay permanent. Fastmail documents its limits but not its reply text.
- Graph unchanged: `sendMail` answers 202 before transport, a sending limit returns as a bounce, and 429 was transient already.
- A not-attempted send now names why the channel stopped: `the send channel broke: ...`, or `a sending limit holds the channel: ...`.

## Left

- No ceiling: a misread refusal holds the queue, each run exiting 3 with the provider's reply, until a person cancels the row.
- Each run retries the first held send once; holding the channel until a reset time is a later change if that proves noisy.

Capability moved: sync (a new requirement, "A quota refusal keeps the send queued"; the submit requirement's transient and permanent classes).
