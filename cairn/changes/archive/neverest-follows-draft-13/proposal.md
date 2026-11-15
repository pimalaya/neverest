---
cairn: change
id: neverest-follows-draft-13
status: landed
created: 2026-11-15
---

# neverest follows draft-13

## Why

pimdir draft-12 and draft-13 were tagged on 2026-11-15 (pimdir 1c6442e: storage-, sync- and search-draft-13; draft-12 was never released by io-pimdir), and io-pimdir 0.14.0 follows them. neverest goes from draft-11 (io-pimdir 0.13) straight to draft-13, so the stores are rebuilt once. The drafts bring three rules a sync engine and its connectors meet:

- **A narrowed round proves only what its enumeration lacks** (pimdir SYNC §5, draft-12): a round a date filter narrowed finds absent only a member its last page's `enumerated` lacks, and nothing when it hands none. neverest's band rounds (IMAP, Gmail and Graph) handed none, so a message deleted inside a widened band would stay in the store (vectors 160 and 161). Line N27 of the CLI alignment round.
- **A pending create is withdrawn whatever the rights** (pimdir STORAGE §15.6, draft-12): io-pimdir's gate; neverest's Graph add outside Drafts, never pushed, can now be removed by a frontend with `item.delete` off.
- **A refused push shows on its receipt** (pimdir SYNC §4, §5, STORAGE §15.4, draft-13): a connector answers `Refused { reason }` for a change the remote refuses as it stands; the engine records it on the binding, holds the change while its key stands, and reports it every run (`PimdirSyncEvent::Refused`, `PimdirSyncReport::refused`). Line N30: neverest's connectors answered `Rejected` for every failure, so Graph's "appends to Drafts only" and Google's floating-time 400 were pushed again on every run.

## What

- Cargo.toml: `io-pimdir = "0.14.0"` from crates.io (first through a git pin at d184622, dropped once 0.14.0 was published), with the X14 dependency pass: pimalaya-cli 0.6.0, io-imap 0.11.2, io-gpeople 0.8.1, io-msgraph 0.9.2, io-gcal 0.5.1.
- N27: the last page of a narrowed round hands the enumeration whether it is a band's or not: IMAP's `UID SEARCH ALL`, Gmail's label ids listed with no query, and for Graph, whose scoped round's pass already enumerates, a fresh delta of the folder with the pass's `$select`, its link dropped (src/msgraph/client/mail.rs `folder_ids`).
- N30: `crate::error::refusal` reads an error chain as a refusal as it stands: a coded `unsupported` (a rule neverest states before any request), or a protocol's `refuses` (an HTTP status from 400 to 499 but 401, 407, 408, 409, 412, 423, 425 and 429 on DAV, Graph, Google Calendar, People and Gmail; an IMAP `NO` naming `CANNOT`, `PARSE`, `TOOBIG` or `NOPERM`), never a quota nor a rate limit. `PimRemote::fail` answers `Refused { reason }` for those, `Rejected` otherwise, on every write of every connector.
- Output: the engine's `Refused` events land in the report under `refusals` (`side`, `collection`, `id`, `reason`), a warning, once a run, the hunk taken back; `sync --json` prints a `refused` event for each once the account ends.
- The store refusal names the rebuild (`neverest init`, then a sync); the tests naming the stranded draft name draft-11.

## Deviations from the brief

- **401 and 407 are not refusals**, nor 423: they name the credentials or a lock that passes, and a refusal holds the change until the item changes, so a token expiring mid-run would hold every change it met for good.
- **A quota is not a refusal**, whatever its status (a DAV 403 `quota-not-exceeded`, Graph's 403 `ErrorQuotaExceeded`, IMAP `OVERQUOTA`), though pimdir SYNC §4 lists "a size or a quota exceeded": freeing space would not release a held change, which only an edit of the item does.
- **Graph's band round hands its enumeration too**, beyond the IMAP and Gmail the line names: it handed none, and its test of a band member deleted before the round closed failed on draft-12. The cost is one fresh delta over the folder per band round (ids, dates and flags).
- **No neverest sync vector runner exists**: the engine is io-pimdir's, whose tests/vectors_sync.rs runs format 7 (cases 162 to 167). neverest's own regression tests drive its connectors and its driver over scripted servers instead.
