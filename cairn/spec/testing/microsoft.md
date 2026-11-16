# neverest on Microsoft Graph (two-way): test report

- neverest: v0.3.0 (rev 59ca533, debug build of 2026-11-15, run from a frozen copy); pimdir CLI io-pimdir v0.13.0
- account: `microsoft` in ~/.neverestrc, two-way into the store; sources `msgraph`, `msgraph-contacts`, `msgraph-calendar`, bearer token from ortie's `microsoft-graph` account; Graph sends by itself (no SMTP channel); `item.delete = false` on every source
- frontends: himalaya v2.2.1 (60e48de8), cardamum v0.3.0 (601f8b9), calendula v0.2.0 (a1ca6dd), direct (`-a microsoft`) and through the store (`-a microsoft-pimdir`)
- date: 2026-11-15, 12:02Z to 13:45Z
- campaign: .github/cairn/changes/live-testing-2026-11-15, part 1; logs in the session's scratchpad (logs/microsoft/); the corpus: corpus.md
- run-made data: mail folders `Pimalaya-Test-A` and `-B` (drafts `pimalaya-test-n7-microsoft-1`, `-2`; the rejected `n8-microsoft-1` lives in the store only); contact folder `Pimalaya-Test-A` (cards `pimalaya-test-n7-microsoft`, `n8-microsoft`); calendar `Pimalaya-Test-A` (two events); `n11-microsoft-send` (Inbox and Sent Items)

## Results

| # | Step | Result |
| --- | --- | --- |
| N1 | probe | ✅ exit 0; msgraph 12 folders with roles (archive, trash, drafts, inbox, junk, sent), msgraph-calendar 8 calendars, msgraph-contacts 2 folders; `mail.message.add` declared `partial` ("Drafts only") |
| N2 | init | ✅ exit 0; second `init` `alreadyInitialized` |
| N3 | declare | ✅ 22 collections |
| N4 | dry run | ✅ only `fetch` hunks: 3,848 messages, 239 events (equal to the corpus), 50 cards (equal) |
| N5 | first sync | ✅ completing run exit 0 in 338 s after the N13 interruptions; 1.16 GB downloaded; every folder, calendar and contact folder equal to the corpus but Deleted Items, 26 where corpus.md says 835: the 835 is Graph's `totalItemCount`, which counts the deleted contacts and events the folder also holds; `envelope list` and the message delta both find 26 messages (a himalaya finding, see Observations) |
| N6 | idempotence | ✅ exit 0, empty patch |
| N7 | remote to store | ✅ two drafts made in Pimalaya-Test-A (Graph creates every MIME message as a draft: `message add` refuses, `msgraph messages create -f <folder>` makes them), one flagged, a card and an event created, all synced; a move A to B kept the item (6369); the event edit arrived; the card edit had to go through `cardamum msgraph contacts update` (cardamum's unified `card update` fails on Graph) and arrived. Re-run at cardamum 9377aff (unreleased fix): `cardamum -a microsoft card update -k <folder> <id> --full-name …` exits 0, Graph shows the new name and keeps the note |
| N8 | store to remote | ⚠️ through `microsoft-pimdir`: the flag, the card create and edit, the event create and edit reached Graph. The message add was accepted by himalaya, applied by the drain, then rejected by every sync since ("Graph creates every MIME message as a draft, so it appends to Drafts only, not to Pimalaya-Test-A"), exit 3 on every run, and it cannot be deleted locally (**F2**) |
| N9 | deletes | ⏸ not run (owner's step, see icloud.md) |
| N10 | merge and conflict | ✅ card: displayName on Graph and NOTE in the store merged (Graph shows both, NOTE as personalNotes); the same field parked, `--prefer-remote` settled it. Event: SUMMARY and LOCATION merged; SUMMARY two ways parked, `--prefer-local` pushed |
| N11 | send | ✅ submitted once through Graph's sendMail; one copy in Inbox and one in Sent Items, neverest appending none (Graph files its own) |
| N13 | interruption | ✅ the first sync interrupted twice by SIGINT: it stopped about 20 s after the signal with `interrupted: true` and no failed hunk, exit 4 (read with `timeout --preserve-status`); the next run resumed from its cursor and finished with every count equal to the server, no duplicate |
| N14 | scope | ❌ `--since 30d --source msgraph` changed nothing; the unscoped run then unbound 24 messages of Sent Items that Graph still holds (store 422, server 446) (**F1**) |
| N15 | index | ✅ `pimdir search` from himalaya (5 hits, all microsoft) and calendula |
| N16 | all accounts | ⚠️ microsoft exits 3 in every run (F2) |

## Findings

### Bugs / issues

- **F1 (neverest, high): a widening round unbinds messages whose `Date` lies far from Graph's `sentDateTime`.** After the scoped run, the widening round "open a round over PimdirScope { since: None, until: Some(2026-10-16) }" on Sent Items reconciled "24 pulled", 24 `delete` hunks. The 24 are io-smtp CI mails whose Graph `sentDateTime` is 2026-10-16 or 2026-11-02 (Exchange stamps the submission) while their `Date` header, which the store scopes by, is much older. Graph narrows by `sentDateTime`, the engine judges absence by `Date`, so they are absent from a listing that should have held them. A later sync never brings them back. Same class as Gmail's (google.md F1); handed to the CLI alignment session as N17.
- **F2 (neverest and himalaya, medium): a message added to a non-Drafts Graph folder through the store is a dead end.** (a) neverest declares only a source-wide `mail.message.add: partial` row (pimdir.db `capabilities`, collection NULL), so himalaya cannot refuse the add up front; (b) every sync rejects it again and exits 3, with `code: null`; (c) `himalaya -a microsoft-pimdir message delete` of the never-pushed item stages a move to Deleted Items and is refused ("does not support mail.message.move: item.create or item.delete is disabled"), so nothing undoes it while deletes are off. Handed over as N15 and H16. Regression tests: an add to a non-Drafts Graph collection refused at enqueue; a never-pushed item deleted locally with `item.delete` false.

### Observations

- himalaya's `mailbox list --counts` on Graph reports `totalItemCount` (Deleted Items 835) where `envelope list` finds 26 messages: reported to the CLI alignment session as a himalaya defect, fixed in himalaya be5beee5 (unreleased) and re-run live on 2026-11-15: Deleted Items [26, 5] where the release says [840, 5]; the unread count was right all along.
- Graph rewrites an event body on write: after an update is pushed, the next run fetches the event back once.
- Several CI leftovers live on the account (an empty neverest-live calendar, three io-msgraph-test calendars); they sync like the rest.

## Verdict

Graph syncs mail, contacts and calendars both ways correctly, survives interruption and resumes without duplicates, and merges and settles conflicts. Two defects: the scope widening loses messages the server holds (F1), and an add to a non-Drafts folder can neither succeed nor be withdrawn (F2). Deletes (N9) wait for the owner.

## Re-run on the fix (2026-11-15, 14:42Z)

neverest efc2cb3 (unreleased), on the same store: `sync --since 30d --source msgraph` then `sync --source msgraph`. Sent Items went from 425 to 448, the server's count: the 24 messages F1 had unbound are back. The scoped run's two deletes are run-made messages the himalaya cell had removed on the server (one kept retained, `pimalaya-test-himalaya-m-send1`), not losses. Both runs exit 3, on F2 alone: the two appends into non-Drafts folders, now coded `unsupported` (F2 (b) fixed), still in the store, since the never-pushed item cannot be withdrawn (F2 (c), an io-pimdir rule, being fixed there). F1 is fixed in efc2cb3.
