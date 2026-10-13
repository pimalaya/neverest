---
cairn: log
change: gmail-reads-in-paced-batches
date: 2026-10-13
---

# Gmail reads in paced batches, once a run

The Gmail source read one message a request, latency-bound at about 5 a second, failed the run on Gmail's per-minute quota refusal, which its throttle did not recognise, and read a message once per label it carried, the history once per label too. It now reads in batches of 10 through Gmail's batch endpoint, one batch in flight across the source, paced by the units of all its reads under a sliding minute's budget, and reads each message and the history once a run. The Android client's measurements set the numbers: batches of 10 one after another drew no refusal in 2,250 reads, 50 drew 19 %, two side by side a third to two thirds.

## What landed

- src/throttle.rs (Google part): `google_refusal` classes a Google answer as a burst, a per-minute quota or a daily quota from its reasons, canonical status and wording; `GoogleWaits` turns it into the wait `Throttle::call` takes (1 to 2 s; 2, 3, 4, 5 s then the next minute; Google's midnight, America/Los_Angeles, which gives the source up). `Throttle::paced_per_minute` adds a sliding sixty-second budget to the bucket. `call` itself is unchanged. gcal (message only) and gpeople (reasons) read through it too.
- src/gmail/batch.rs: `read_all` over a `Batcher` trait, ported from the Android client (`rust/src/client/gmail.rs`), with its fake batcher tests and the three refusals Gmail really answered.
- src/gmail/run.rs: what a source's connections share for a run, carried by `GmailAccount`: the settings and the in-flight gate, the bounded metadata and size caches, the mailbox history and its projection on a label.
- src/gmail/client.rs: a second session to `www.googleapis.com` for batches; round pages, deltas, summaries and bodies read through `read_all`; writes on a gone message converge; tests over scripted sessions (a message under two labels read once, the history read once with no label, bodies sized by the run's reads, writes on gone messages).
- src/config.rs: `gmail.batch-size` (10, 1 to 100), `gmail.batches-in-flight` (1), `gmail.units-per-minute` (6,000), refused out of range when the account resolves.

## Left

- One download for a message under two labels, sizes from the store, and a gone body told to the driver: driver work, described in the proposal.
- A live run on the test account.

## Capabilities moved

**sync**: the throttling requirement (Google refusals by reason), Gmail pacing (the minute's budget), Gmail reads in paced batches (added), and the Gmail source's delta (history once a run) and writes (a gone message converges).
