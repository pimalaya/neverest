---
cairn: change
id: a-send-is-handed-over-once
status: landed
created: 2026-10-13
---

# A send is handed over once, and can run alone

## Why

Reported by MOA, which sends by `himalaya pimdir message send --save <sent>` (a queued `submit`), then runs `neverest sync -a <account> --json --include-collection <sent>` and reads `submitted[]`:

- **A send waits for a sync.** The only way to perform a queued `submit` was a sync, which went on to list and reconcile the Sent folder after the send. On a large Sent folder that took longer than MOA's budget, and MOA killed the run at its deadline.
- **A send could leave twice.** Submission was at-least-once: a failure with no reply was transient whatever its moment, so a message the server took and whose `250` was lost (a dropped link after the final `.`, a Graph `sendMail` answered by a gateway 503, a read timeout) stayed pending and the next run sent it again. A run killed between the server taking the message and the acknowledgement did the same. MOA, unable to tell a message never handed over from one maybe delivered, read every such failure as "maybe sent".
- **A send channel that did not open was silent.** An SMTP server that could not be reached was a warning only: no `unreached`, no exit 3, no entry in `submitted`, so a caller saw a report with nothing in it and guessed.

The Android client settled the same questions in write-once-and-oauth-dos (android cairn): a send failing after the whole message was handed over and before any verdict is *uncertain*, parked, never sent again on its own.

## What

**`sync --intents-only`.** Applies the queue, then performs what frontends queued (collection creations, sends, calendar invitation replies and cancellations) and stops: no collection is listed, no other write pushed, nothing fetched, no retention sweep. An endpoint opens only when one of these needs its session: an intent it performs on its server, a send through its native channel (Graph). A send through an `smtp` table opens that channel alone, so a message leaves while the source's own server is down. Named after `--declare-only`, its sibling, rather than `--submit-only`: it performs every intent kind, which is what a frontend queueing a reply to an invitation wants too, and a send-only flag would have to be followed by another run for those. It refuses `--dry-run`, `--reset`, `--declare-only`, `--since` and the collection filters (`-m`, `-x`, `-A`), which mean nothing to a run listing no collection; `--source` narrows it as it narrows a sync. Exit codes as a sync: 0, 2 when a send was parked as uncertain, 3 when an endpoint was unreached or an intent failed without parking. A sent copy neverest files (not Gmail, not Graph) is queued and drained into the store at once, and pushed by the next sync.

**Uncertain sends.** `SubmitFailure::Uncertain`, beside `Transient` and `Permanent`:

- SMTP is driven by neverest over io-smtp's `SmtpMessageSend` coroutine, which tells where a failure fell: a 5yz reply is permanent, a 4yz reply transient (a verdict: not taken), and a failure with no reply transient before the data's final `.` is written and flushed, uncertain after it. A refusal resets the transaction (`RSET`) before the next message; a failure with no reply loses the session, and the intents behind are not attempted this run.
- Graph `sendMail` runs through the session's one exchange layer (src/msgraph/client/wire.rs), whose record now also says whether the request was flushed whole before its first read (`Exchange::written`); as every request that writes, it is never sent again on a dead connection, and only a 429, which Graph answers before acting, is sent again: 400, 403, 404, 413 and 422 permanent; 401, 408 and 429 transient; 500, 502, 503 and 504 uncertain; any other 4xx permanent and anything else transient; with no status (an answer cut short or unparsable included), uncertain once the request was written whole, transient before.
- Neverest has no Gmail API send: Gmail sends through its `smtp` table, classified as SMTP.

An uncertain send is parked with an error beginning `uncertain: ` and never sent again by any run; only a person, through the producer, sends it again. `submitted[]` entries gain `uncertain: bool`, true only for such a send. The run that parks one exits 2.

**Killed runs.** Before the first byte of a message leaves, its attempt is counted (`attempts` in `pimdir queue show`) and its queue id written to `submitting` beside `pimdir.db` (written aside, synced, renamed). The mark is cleared once the outcome is in the store. A run finding a mark parks that intent as uncertain before sending anything. A store recreated (`init`, `--reset`) clears it.

**An SMTP channel that does not open.** A server that cannot be reached, or a session cut before its credentials were answered, is named under `unreached` as `<source>.smtp`, the intents pending and unattempted, the run exiting 3 (a send channel has no `*` scan to fail, so `unreached` alone now makes a run incomplete). Credentials the server refused are not an unreached endpoint: each queued send is reported failed with the reason, not parked, not attempted, the run exiting 3.

## The window that remains

A killed run (SIGKILL at a caller's deadline) can no longer send a message twice, and the row reads attempted as soon as it may have left. Step by step, for one intent:

1. `fail_action(id, None)` commits `attempts + 1` (SQLite WAL, durable across a kill). Killed here: the row reads pending, `attempts` 1, nothing sent; the next run sends it normally. A caller reading `attempts` 1 cannot tell this from 3–4 and must treat the row as maybe sent; it is the honest answer only one step early.
2. `submitting` is written and renamed. Killed between 1 and 2: as above.
3. `MAIL FROM`, `RCPT TO`, `DATA`, the data (or the Graph `POST`). Killed anywhere here or later, before 5: the mark stays, the next run parks the row as uncertain, never resending it. A message killed before its final `.` was not delivered, and is parked anyway: the price of a mark that cannot know which byte left.
4. The verdict is read.
5. The outcome is committed: `acknowledge_action` / `replace_action` (receipt) for a sent message, `fail_action(id, Some(..))` for a parked one; a transient failure needs nothing more.
6. `submitting` is removed. Killed between 5 and 6: the next run finds the mark but no pending row with its id, and only clears it.

So no window resends a message; the one left is the conservative one, a message killed mid-handover being parked uncertain although the server may never have seen its final `.`. A failure to commit the outcome at 5 keeps the mark and stops the run's sends, so the next run parks the row rather than sends it again.

A parked uncertain send counts two attempts, the one counted before it left and the one `park_action` counts.

## Not done

- No read timeout of its own on the SMTP session: a server holding its verdict holds the run until the caller kills it, which the mark makes safe. Graph keeps its 30-second read timeout, after which the send is uncertain.
- No dedup by `Message-ID` on the server to settle an uncertain send: no SMTP server without its own sent filing could answer, and Gmail rewrites the `Message-ID` (the Android reasoning).
- An `--intents-only` run resolves every credential of the account, as a sync does (`Account::resolve`), though it may open none of them.
