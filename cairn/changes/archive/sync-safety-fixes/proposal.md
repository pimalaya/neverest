---
cairn: change
id: sync-safety-fixes
status: landed
created: 2026-10-07
---

# Sync safety fixes

## Why

Block 1 of the joint plan for scoped mail sync (pimdir `cairn/changes/scoped-mail-sync`, §8, "Fixes that need no spec, first"): four defects found while planning paged, scoped mail sync, each wrong today whatever the plan becomes.

- **Graph dates.** The Graph mail source stored `receivedDateTime` under pimdir's mail `date`, which Annex A.1 defines as the `Date` header and which the mail sort key reads; a mail moved or delivered late sorted by its arrival.
- **IMAP deletes.** A delete marked the UID `\Deleted` then sent a plain `EXPUNGE`, which removes every message any client marked `\Deleted` in that mailbox.
- **Throttling.** Apart from Graph's batched body fetches, a 429, a 503 or a Google rate limit failed the request at once, so a large first sync ran into its quota and lost the rest of the run to errors, with nothing in the report saying why.
- **Gmail pacing.** Gmail meters each user at 250 quota units a second (`messages.list` and `messages.get` cost 5): a source fetching as fast as it can runs into that quota instead of staying under it.

## What

- Graph: `$select` asks for `sentDateTime`, the summary's date is it; absent, no date (Annex A.1: `NULL`), never the arrival time.
- IMAP: `UID EXPUNGE` on the one UID (RFC 4315, IMAP4rev2). A server offering neither UIDPLUS nor IMAP4rev2 gets no delete: the push fails and stays in the store, no `\Deleted` flag stored.
- A shared per-endpoint throttle: Graph, Gmail, Google Calendar and People, CalDAV and CardDAV send a throttled request again after the stated wait or a bounded exponential back-off with jitter, then give up; every connection of the source stops until the wait is over, what landed stays, the report lists `throttled: [{ source, until }]` and the run exits 3.
- Gmail: a token bucket of 200 units a second shared by the source's connections, each call taking its listed cost.

## Not done

- `Retry-After` outside a Graph batch: io-msgraph, io-webdav, io-gmail, io-gcal and io-gpeople keep no response header on their errors, so the wait there is neverest's back-off (Google's `Retry after <instant>` in a message is read). Each crate would need to carry it.
- Google's error `reason`: the Google crates keep the envelope's status and message only, so a 403 rate limit is read from the message.
- Larger Graph pages (`Prefer: odata.maxpagesize`), scopes and pages: later blocks of the plan.
