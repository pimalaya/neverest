# neverest on Google (two-way): test report

- neverest: v0.3.0 (rev 59ca533, debug build of 2026-11-15, run from a frozen copy); pimdir CLI io-pimdir v0.13.0
- account: `google` in ~/.neverestrc, two-way into the store; sources `gmail` (with an SMTP channel, smtps://smtp.gmail.com:465, XOAUTH2), `gpeople`, `gcal`, bearer token from ortie's `google` account (scopes `https://mail.google.com/`, `calendar`, `contacts`); `item.delete = false` on every source
- frontends: himalaya v2.2.1 (60e48de8), cardamum v0.3.0 (601f8b9), calendula v0.2.0 (a1ca6dd), direct (`-a google`) and through the store (`-a google-pimdir`)
- date: 2026-11-15, 12:05Z to 14:00Z
- campaign: .github/cairn/changes/live-testing-2026-11-15, part 1; logs in the session's scratchpad (logs/google/); the corpus: corpus.md
- run-made data: labels `Pimalaya-Test-A` (Label_486) and `-B` (Label_487); messages `pimalaya-test-n7-google-1`, `-2`, `n8-google-1` (each also under SENT, Gmail filing a message from the account itself there), `n11-google-send` (INBOX and SENT); cards `pimalaya-test-n7-google`, `n8-google` in myContacts; calendar `Pimalaya-Test-A` (c_f85ae77c…@group.calendar.google.com, two events)

## Results

| # | Step | Result |
| --- | --- | --- |
| N1 | probe | ✅ exit 0; gcal 3 calendars, gmail 15 labels (INBOX, SENT, TRASH, DRAFT, SPAM with roles, user labels by name), gpeople 1 (`contacts`); SMTP channel authenticated over XOAUTH2 |
| N2 | init | ✅ |
| N3 | declare | ✅ |
| N4 | dry run | ✅ only `fetch` hunks: 897 Gmail (the sum of the label counts), 289 events (66 + 209 + 14), 33 contacts; 104 s, Gmail's metadata reads paced |
| N5 | first sync | ✅ exit 0 in 367 s; every label, calendar and the contact book equal to the corpus |
| N6 | idempotence | ✅ exit 0 in 47 s, empty patch |
| N7 | remote to store | ✅ labels made, two messages imported (one `\Seen`), one flagged, a marked card and a run-made calendar with an event; synced; a move A to B kept the item (6374); the event edit arrived at once; the card edit arrived one run late (Google People's listing lags a write, see Provider behaviour) |
| N8 | store to remote | ✅ through `google-pimdir`: flag, message add, card create and edit, event create and edit; drain applied six rows, the sync pushed them, Google shows each |
| N9 | deletes | ⏸ not run (owner's step, see icloud.md) |
| N10 | merge and conflict | ✅ card: FN on Google, NOTE in the store; the first sync pushed the local edit against a stale etag and Google refused it (HTTP 400 "Request person.etag is different than the current person.etag"), nothing overwritten; the next run merged both. FN two ways parked, `--prefer-remote` settled it. Event: merged, then parked and `--prefer-local` pushed |
| N11 | send | ✅ submitted once through SMTP (XOAUTH2); one copy in INBOX, one in SENT, neverest appending none (Gmail files its own) |
| N14 | scope | ❌ `--since 30d --source gmail` changed nothing; the unscoped run then unbound 13 messages from INBOX and SENT that Gmail still holds (store INBOX 307 and SENT 82, server 320 and 95) (**F1**) |
| N15 | index | ✅ `pimdir search Pimalaya-Test` from himalaya: 4 hits, all google; cardamum and calendula find the run-made cards and events |
| N16 | all accounts | ✅ google's turn in the run over the four accounts is clean (exit 0 for the account) |

## Findings

### Bugs / issues

- **F1 (neverest, high): a Gmail widening round unbinds messages dated long before their arrival.** The widening round "open a round over PimdirScope { since: None, until: Some(2026-10-16) }" narrows Gmail with `before:`, which reads the arrival; 13 io-smtp CI mails dated 2026-01-01 (store sort key 2026-01-01T00:00:00Z, Message-IDs …@mx.google.com) arrived recently, so the listing missed them and the round's last page "reconciled: 13 pulled": 13 `delete` hunks in INBOX and 13 in SENT. The code assumes a band round needs no enumeration (src/gmail/client.rs:834-848: its NOTE covers a `Date` running ahead of the arrival, not behind it). Gmail still holds every one; a later sync never brings them back (the history delta does not re-list them). Same class as Graph's (microsoft.md F1); handed over as N17.
- **F2 (neverest, low): the "Holidays in France" calendar's sync token is refused on every pass.** The token the first pass of a run takes is called expired by the second pass of the same run seconds later ("calendar sync token of en-gb.french#holiday@group.v.calendar.google.com expired, restarting a full listing", WARN, twice a run), so the 209 events are re-listed on every pass. The other calendars keep their tokens. Handed over as N16.

### Provider behaviour (not bugs)

- Google People's listing lags a write by seconds: a sync right after a remote edit misses it, the next one sees it. With a local edit pending, the push meets Google's etag check and is refused, losing nothing.
- Gmail files a message whose `From` is the account under SENT as well, so run-made messages appear in a corpus label.

### Observations

- A flag set through one label's binding is pushed again on the message's other bindings a run later (an `add-flags` on SENT for a message flagged in Pimalaya-Test-A): harmless on Gmail, where flags are per message, one request wasted.
- The rejected row of a stale etag carries `code: null`; the shared code vocabulary (cairn/spec/sync.md, "A server's refusal carries its code") has no code for a stale precondition. A vocabulary question for the owner, not a defect.

## Verdict

Google syncs mail, contacts and calendars both ways, merges, settles conflicts and sends correctly; People's lag costs one run and loses nothing. One high defect (F1) loses messages from the replica when a date-scoped sync is widened, and one low one (F2) re-lists a holiday calendar every pass. Deletes (N9) wait for the owner.

## Re-run on the fix (2026-11-15, 14:40Z)

neverest efc2cb3 (unreleased, built from the commit), on the same store: `sync --since 30d --source gmail` then `sync --source gmail`, exit 0 both, no delete hunk. The widening round re-listed the 13 messages F1 had unbound: the store's INBOX went from 308 to 321 and SENT from 86 live and 13 retained to 99 live and none retained, equal to Gmail's counts at that time (corpus plus the run's messages). F1 is fixed in efc2cb3. F2's warning is now logged at debug.
