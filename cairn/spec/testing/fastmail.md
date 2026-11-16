---
cairn: spec
capability: testing-fastmail
status: current
---

# neverest on Fastmail (one-way, pull-only): test report

- neverest: v0.3.0 `+dav +gcal +gmail +gpeople +imap +msgraph +rustls-ring +search +smtp +vendored` (rev 59ca533, debug build of 2026-11-15, copied aside so concurrent builds could not change it)
- pimdir CLI: io-pimdir v0.13.0 (`./tests/pimdir.sh`), read-only verbs only
- account: `fastmail` in ~/.neverestrc, `one-way = true`; sources `imap` (imaps://imap.fastmail.com), `carddav` (https://carddav.fastmail.com/dav/), `caldav` (https://caldav.fastmail.com/dav/), every write gate closed (`collection.create`, `flag.update`, `item.create`, `item.delete`, `item.update` all false), no SMTP channel
- store: ~/.local/state/neverest, created by N2, fastmail its only account so far
- date: 2026-11-15, 11:48Z to 11:56Z
- campaign: .github/cairn/changes/live-testing-2026-11-15, part 1, steps N1 to N6; logs in the session's scratchpad (logs/n/)

## Results

| # | Step | Command | Result |
| --- | --- | --- | --- |
| N1 | probe | `neverest -a fastmail check`, then `--json check` | ✅ exit 0 both; mode `{sources: [caldav, carddav, imap], targets: [], oneWay: true, retain: true}`; caldav 2 collections (`Pimalaya`, `test`, the latter taking tasks), carddav 1 (`Default`, named `Personal`), imap 7 with their roles (INBOX inbox, Archive archive, Drafts drafts, Sent sent, Spam junk, Trash trash, Scheduled none); every write capability `none` ("one-way copy: the server wins") but `collection.rename` (**F1**) |
| N2 | init | `neverest -a fastmail init`, then `--json init` | ✅ exit 0 both; the first creates the store and initializes the three sources, the second answers `alreadyInitialized: true` |
| N3 | declare | `neverest -a fastmail --json sync --declare-only` | ✅ exit 0; events then one result, an empty patch; `pimdir collection list` holds the same 10 collections as N1, under `fastmail/<source>/<id>`, with kind, display name and role; no item, no object |
| N4 | dry run | `neverest -a fastmail --json sync --dry-run` | ✅ exit 0; 53 `fetch` hunks (INBOX 31, Sent 10, Drafts 1, carddav Default 7, caldav Pimalaya 4), each equal to the server's count read through himalaya, cardamum and calendula at that moment; 12,659 octets of IMAP headers listed; the store unchanged (0 items, last change 26) |
| N5 | first sync | `neverest -a fastmail --json sync --log-level debug --log-file …` | ✅ exit 0 in 6 s; the 53 hunks applied, none failed; no conflict, refusal, rejection, absence, waiting or unlisted row; coverage for all 10 collections; 713,578 octets downloaded from IMAP, 1,765 from CardDAV, 3,627 from CalDAV; the index folded 55 changes, 53 items indexed, 0 behind. Store: 55 live items (53 plus 2 in the attachments collection, **O1**), 53 objects, 689.8 KiB, every collection's count equal to N4's |
| N6 | idempotence | the same command, then a third run in text | ✅ exit 0 both; an empty patch, nothing downloaded, the store unchanged (last change 188); the text run ends "Account fastmail is already in sync" |
| N12 | one-way | through `fastmail-pimdir`: `himalaya flag add`, `himalaya message add -m fastmail/imap/Drafts`, `cardamum card create -k fastmail/carddav/Default`, then `drain` and `sync` | ✅ each write refused before it is queued, naming why ("Source fastmail/imap does not support mail.flags.flagged: one-way copy: the server wins", the add and the card coded `unsupported`); the drain applied nothing, the sync pushed nothing; Fastmail's counts, INBOX flags and card etags identical before and after |
| N13 | interruption | the sync in a network namespace with no interface (`unshare --user --map-current-user --net`) | ✅ exit 4, `unreached` names caldav, carddav and imap |
| N14 | scope | `sync --since 30d --source imap`, then `sync --source imap` | ✅ both exit 0, nothing changed: 42 IMAP items before and after (Fastmail answers the undated `NOT HEADER Date ""` clause that iCloud refuses) |
| N15 | index | `himalaya -a fastmail-pimdir pimdir search Pimalaya-Test` | ✅ answers from the store (two older Fastmail messages matching both words) |
| N16 | all accounts | `neverest --json sync` twice over the four accounts | ✅ fastmail runs first and is empty both times |

## Was anything written to Fastmail

No sign of it. Every hunk of N4 and N5 was a `fetch`, every write gate was closed, and the server's counts after N6 equal N4's (INBOX 31, Archive 0, Drafts 1, Scheduled 0, Sent 10, Spam 0, Trash 0). INBOX still holds 19 unread messages after all 31 bodies were downloaded, so the body fetches did not set `\Seen`. The debug logs cannot settle it either way: they record no protocol command at all, reads included, so a count of write verbs in them proves nothing, and a trace log was not taken because it would carry the IMAP credentials.

## Findings

### Bugs / issues

- **F1, a one-way source still declares `collection.rename` as `full`.** N1 prints `collection.rename: full` on caldav, carddav and imap, beside `none` ("one-way copy: the server wins") for every other write. The spec says "a one-way source SHALL support no write" (cairn/spec/sync.md, `Every source declares its capabilities before the drain`). The code lets intents through on a one-way source on purpose (src/sync/capability.rs `supports`, and the test `a_one_way_source_refuses_writes_but_not_intents`), and `collection.create` and `collection.delete` stay `none` here only because their config gates are off. `collection.rename` has no such gate, so no configuration can stop a frontend's rename intent from renaming a mailbox, address book or calendar on a pull-only server. Not exercised in this run (no frontend queued anything). For the owner: either a rename gate (`collection.rename`, default true) or a one-way source refusing rename, and the spec and the code made to say the same thing. Tracked as D14 in .github/cairn/changes/cli-alignment-2026-11-15; decided by the owner on 2026-11-15: a `collection.rename` permission beside create and delete, false by default on a one-way account and true otherwise. To re-run N1 on the commit that lands it: `collection.rename` must read `none` on all three fastmail sources.
- **F2, progress lines carry terminal colour codes on a stderr that is no terminal.** `neverest -a fastmail check 2>file` writes four `ESC[38;5;10m` sequences into the file, and `NO_COLOR=1` changes nothing. himalaya writes none in the same situation. Low; a family CLI matter (pimalaya-cli progress), handed to the CLI alignment session (its N13).

### Observations

- **O1, the attachments collection.** N5 adds `fastmail/\x01attachments` (kind `application/octet-stream`, role `attachments`, 2 live items), the reserved 0x01 prefix read in pimdir.db; the pimdir CLI prints that byte as U+FFFD. Part 2 checks how himalaya's pimdir backend shows it.
- **O2, `--json` streams events, then one result**, as the CLI output contract says: N3 printed 7 events before its result, N5 printed 20 (account-started, opened and listed for each source, 13 scan-progressed).
- **O3, the logs hold no secret.** The three debug logs were searched for an `Authorization` header, a bearer or basic credential and a password: none. IMAP logs `authenticate with plain` and nothing more.

## Verdict

Part 1 holds on Fastmail: a one-way account probes, initializes, declares, plans, fills and settles exactly, its counts equal to the server's, and nothing was written there. One spec and code disagreement (F1) matters to every pull-only account and needs the owner's decision; F2 is cosmetic.

## Re-run on the fix (2026-11-15, 14:37Z)

neverest efc2cb3 (unreleased): `neverest -a fastmail --json check` reads `collection.rename` at level `none` on caldav, carddav and imap, with the detail "one-way copy: collection.rename is off unless set to true on the source". F1 is fixed as the owner decided (D14).
