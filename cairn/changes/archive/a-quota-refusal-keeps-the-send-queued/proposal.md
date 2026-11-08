---
cairn: change
id: a-quota-refusal-keeps-the-send-queued
status: landed
created: 2026-11-08
---

# A quota refusal keeps the send queued

## Why

The Android app parked a send its provider refused for a rate or a daily quota as refused for good, so the message waited for the user's *Send again* when it only had to wait (android cairn/log/2026-11-06-a-send-refused-for-a-rate-stays-queued.md). neverest's SMTP channel has the same shape: `smtp_disposition` (src/offline/submit.rs) parks every 5xx as permanent, and Gmail answers its daily sending limit with `550 5.4.5` (Google's SMTP error reference). A row held by a quota parks, and only a person queues it again, though the next day's run would have sent it.

Found by reading the code against the Android fix on 2026-11-08 (neverest d669915), not reproduced; the `550 5.4.5` reply is from Google's documentation, not seen live.

## What

- An SMTP rejection is read with its reply text, which io-smtp 0.6 carries beside the code (`Rejected { code, message }`), not by its code alone.
- A 5xx refusing for a rate or a quota is transient: the row stays pending, reported under `submitted` with its error and `parked: false`. It is recognised by the enhanced status code opening its text when that code means nothing else (`5.4.5`, `5.1.90`, `5.7.232`, `5.7.233`, `5.7.236`), else by wording the providers document under codes of other meanings (`SubmissionQuotaExceeded`, `submission quota`, `sending limit exceeded`, `rate limited`, `daily limit for message recipients`). RFC 3463 registers no code for a sender quota: `5.4.5` is congestion, which it calls a persistent transient error.
- Such a refusal leaves the rest of the run's sends unattempted and pending, as a failure with no reply does: the quota holds for the account, so each further send would only be refused too.
- Every other reply keeps today's disposition, a sender blocked until a person releases it (`5.1.8`, `5.7.501` to `5.7.503`, `5.7.70x`) and the sender's own full mailbox (`5.2.2 ... QuotaExceededException`) included. Graph is unchanged: `sendMail` answers 202 before transport, so a sending limit comes back as a bounce in the inbox, and its request throttle (429) is transient already.

## Out of scope

Waiting until the quota resets: each run retries the first pending send and is refused once. A later change may hold the channel until the next UTC day if that proves noisy.

A ceiling on how long a quota refusal holds a send (parking it past a few days) is left out: it would rest on the row's `created_at` and an arbitrary bound, and a held send is never silent, each run reporting it with the provider's reply and exiting 3.

## Compatibility

Not breaking. A send that parked on a quota now stays pending, and the run ends as a run with a transient send failure does today.
