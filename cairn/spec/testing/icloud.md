# neverest on iCloud (two-way): test report

- neverest: v0.3.0 (rev 59ca533, debug build of 2026-11-15, run from a frozen copy); pimdir CLI io-pimdir v0.13.0
- account: `icloud` in ~/.neverestrc, two-way into the store; sources `imap` (imaps://imap.mail.me.com:993) with an SMTP channel (smtp.mail.me.com:587, STARTTLS), `carddav` (https://contacts.icloud.com/), `caldav` (https://caldav.icloud.com/); `item.delete = false` on every source
- frontends for N7, N8, N10, N11: himalaya v2.2.1 (60e48de8), cardamum v0.3.0 (601f8b9), calendula v0.2.0 (a1ca6dd), direct (`-a icloud`) and through the store (`-a icloud-pimdir`)
- date: 2026-11-15, 12:00Z to 13:40Z
- campaign: .github/cairn/changes/live-testing-2026-11-15, part 1; logs in the session's scratchpad (logs/icloud/)
- run-made data: mailboxes `Pimalaya-Test-A` and `-B`; calendar `pimalaya-test-a`; cards `pimalaya-test-n7-icloud` and `pimalaya-test-n8-icloud` in `card`; messages `pimalaya-test-n7-icloud-1`, `-2`, `n8-icloud-1`, `n11-icloud-send` (INBOX and Sent Messages), `n11-icloud-refused` (Sent Messages, and a bounce to come)

## Results

| # | Step | Result |
| --- | --- | --- |
| N1 | probe | ✅ exit 0; caldav 3 calendars (Rappels, Domicile, Travail, the last the default), carddav 1 (`card`), imap 6 (INBOX inbox, Sent Messages sent, Deleted Messages trash; Archive, Junk and Drafts carry no role, iCloud advertising none, as himalaya also reads it); SMTP channel authenticated |
| N2 | init | ✅ exit 0; a second `init` answers `alreadyInitialized` |
| N3 | declare | ✅ 10 collections recorded with kind, name and role |
| N4 | dry run | ✅ 11 `fetch` hunks only (Domicile 3, card 6, INBOX 1, Sent Messages 1), equal to the server's counts; the store unchanged |
| N5 | first sync | ✅ exit 0 in 8 s; 11 items, every collection equal to the server; index 11 |
| N6 | idempotence | ✅ exit 0, empty patch, nothing downloaded |
| N7 | remote to store | ✅ (first rounds only, see F2) two messages added to Pimalaya-Test-A (one `\Seen`), one flagged, a marked card and an event created, synced with their flags; then a move A to B (the item kept its id, 6367: a `delete` of the old UID in A and a `fetch` in B), the card and the event edited, each new body in the store |
| N8 | store to remote | ✅ through `icloud-pimdir`: a flag, a message add, a card create and edit, an event create and edit; `neverest drain` applied the six queue rows offline (created items got their ids), the sync pushed six hunks, the server shows each. A move through the store is refused while `item.delete` is off ("Source icloud/imap does not support mail.message.move: item.create or item.delete is disabled"), as declared |
| N9 | deletes | ⏸ not run: flipping `item.delete` is the owner's step, and the tool permission layer refused this session's edit of ~/.neverestrc |
| N10 | merge and conflict | ✅ card: FN edited on the server, NOTE in the store, merged with no conflict; then FN two ways, parked (exit 3), `conflict list` and `conflict show` (base, local, remote), `conflict resolve --prefer-remote`, next sync exit 0, both sides equal. Event: SUMMARY on the server, LOCATION in the store, merged; SUMMARY two ways parked, `--prefer-local` pushed the store's body |
| N11 | send | ✅ `himalaya -a icloud-pimdir pimdir message send --save "icloud/imap/Sent Messages"`, `sync --intents-only` submitted it once through SMTP; delivered once to INBOX; exactly one copy in Sent Messages (neverest appends it, iCloud's SMTP files none). A recipient at a reserved `.invalid` domain was accepted by iCloud at RCPT (it bounces later), so the park path could not be exercised live |
| N13 | interruption | ✅ offline (a network namespace with no interface, the user's uid kept so gpg answers): exit 4, `unreached` names caldav, carddav and imap ("Network is unreachable"). The interruption half ran on microsoft |
| N14 | scope | ❌ the scoped round (`--since 30d --source imap`) changed nothing, but the widening round fails on every IMAP collection: "IMAP SEARCH failed: NO Unexpected exception" (**F1**); every later icloud run exits 4 |
| N15 | index | ✅ `himalaya -a icloud-pimdir pimdir search Pimalaya-Test` 5 hits, all icloud; `cardamum -a icloud-pimdir pimdir search Pimalaya` the two marked cards |
| N16 | all accounts | ⚠️ the run over the four accounts reaches icloud in its turn; icloud fails its four IMAP scans (F1) and the run exits 4 |

## Findings

### Bugs / issues

- **F1 (neverest, high): iCloud refuses the undated clause of every date-narrowed IMAP round.** neverest searches `OR SENTSINCE <day> NOT HEADER Date ""` (and `OR SENTBEFORE <day> NOT HEADER Date ""` for a widening round), src/imap/backend.rs:481-499. Reproduced read-only through `himalaya -a icloud imap raw` after `EXAMINE INBOX`: `UID SEARCH SENTBEFORE 18-Oct-2026` and `UID SEARCH SENTSINCE 14-Oct-2026` answer OK; `UID SEARCH NOT HEADER Date ""` and both OR forms answer `NO [UNAVAILABLE] Unexpected exception`. So any scoped round on iCloud (a `--since` sync, `item.filter.since`, the widening after one) fails on every collection, and a round left open fails on every later run (exit 4). Handed to the CLI alignment session as its N18 (search the undated messages apart; a NO there means the server cannot tell, so the round concludes no absence). Regression test: a fake IMAP server answering NO to `HEADER Date ""`.

- **F2 (neverest, high): every change to an iCloud mailbox already synced is missed.** iCloud lists QRESYNC in CAPABILITY but answers `ENABLE QRESYNC CONDSTORE` with a bare OK and no `* ENABLED` line, and a QRESYNC EXAMINE then returns no FETCH. neverest throws away the list `enable` returns (src/imap/client.rs:127) and trusts the capability (`supports_qresync`, :145; `select_delta`, :165), so its delta reads nothing and checkpoints past every arrival, flag change and expunge. Live: the server's INBOX holds UIDs 1, 10, 11, 12, 13 (the N11 delivery, a bounce, carillon's test mails); the store's holds UID 1 alone, and the sync right after the N11 delivery had no INBOX hunk. Changes in a mailbox's first round (N7's new mailboxes) arrive, which is why N7 passed. Found through carillon's iCloud watcher, which io-imap's watch broke the same way (fixed in io-imap, uncommitted, change only-what-enabled-lists-is-enabled). Handed to the CLI alignment session for neverest. Regression test: a fake server answering ENABLE with a bare OK; a delta after an EXISTS fetches the new UID.

### Provider behaviour (not bugs)

- iCloud lists QRESYNC but does not enable it: `ENABLE QRESYNC CONDSTORE` answers a bare OK (RFC 5161 §3.1: only what `* ENABLED` lists is enabled).
- iCloud IMAP advertises no special-use role for Archive, Junk and Drafts.
- iCloud's SMTP accepts a recipient at a `.invalid` domain and bounces later.
- iCloud's CalDAV event bodies written by calendula carry a full VTIMEZONE with its historical transitions; merges and conflicts work on them.

### Observations

- The three-way merge holds on both kinds: different fields merge, the same field parks with three readable bodies, and either resolution reaches both sides.
- A rejected or refused row may carry `code: null`: the spec allows it when the failure carries no code ("A failing row carries its code and its causes").

## Verdict

iCloud syncs contacts and calendars both ways correctly, and mail on a first round of each mailbox; merges, conflicts and sends hold. Two high defects on its IMAP: every change to a mailbox already synced is missed (F2), and every date-scoped run fails and leaves the account stuck at exit 4 (F1). Deletes (N9) wait for the owner.

## Re-run on the fix (2026-11-15, 14:38Z)

neverest efc2cb3 (unreleased), on the same store with its round left open: `neverest -a icloud --json sync` exits 0 in 21 s. The refused undated search now warns ("the server refused to search the messages with no Date (IMAP SEARCH failed: NO Unexpected exception), listing the dated ones") and the round goes on: F1 is fixed. The store's INBOX caught up with the server: UIDs 10 to 16 (the N11 delivery, the bounce, carillon's two messages, the io-imap check) are in it: F2 is fixed (QRESYNC used only when `* ENABLED` names it, else rounds). New, low: imap-codec logs `Rectified missing text to "..."` at WARN 92 times a run on iCloud's responses.
