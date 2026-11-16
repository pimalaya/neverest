# The Google and Microsoft corpus: baseline counts

The fake data already on the two test accounts (owner, 2026-11-15) is kept through the live testing campaign (.github/cairn/changes/live-testing-2026-11-15, step P7). These are its counts before any neverest sync of either account. N5 of each account and the campaign's close are read against them.

- date: 2026-11-15, 11:46Z to 11:50Z, before the first neverest sync of google or microsoft
- read through the direct backends: himalaya 2.2.1 (60e48de8) `--json mailbox list --counts`, cardamum 0.3.0 (601f8b9) `card list` paged to its end, calendula 0.2.0 (a1ca6dd) `item list` (raw items, a recurring series once) paged to its end; every listing answered with no error
- **seed** is the owner's corpus and must not move; **CI** is a throwaway collection a credentialed CI suite made and may create, empty or delete at any time (a Google calendar of that kind vanished between two listings minutes apart, Google deleting asynchronously); **system** is a provider's own view or collection. The close compares the seed rows.

## google (google@pimalaya.org)

Mail (Gmail labels; `total` and `unread` as Gmail counts them):

| Label | Name | Class | Total | Unread |
| --- | --- | --- | --- | --- |
| INBOX | INBOX | seed | 319 | 103 |
| SENT | SENT | seed | 91 | 48 |
| TRASH | TRASH | seed | 1 | 0 |
| DRAFT | DRAFT | system | 0 | 0 |
| SPAM | SPAM | system | 0 | 0 |
| STARRED | STARRED | system (a flag in neverest) | 19 | 2 |
| UNREAD | UNREAD | system (a flag in neverest) | 117 | 117 |
| IMPORTANT, CHAT, CATEGORY_* | | system, not synced | 0 | 0 |
| Label_97 | moa-seed | seed | 399 | 46 |
| Label_98 | Seed | seed | 0 | 0 |
| Label_99 | Seed/Clients | seed | 27 | 3 |
| Label_100 | Seed/Factures | seed | 26 | 6 |
| Label_101 | Seed/Projets | seed | 27 | 1 |
| Label_4324330986265257883 | Demo | seed (origin unknown, kept) | 1 | 0 |
| Label_23 | io-imap-test-1790887631591-renamed | CI | 3 | 1 |
| Label_24 | io-imap-test-1790887631591-copied | CI | 0 | 0 |
| Label_187 | io-imap-test-1791715599257-renamed | CI | 1 | 0 |
| Label_188 | io-imap-test-1791715599257-copied | CI | 2 | 0 |

Contacts (cardamum's view: Google People contact groups; neverest syncs every connection as one address book, `contacts`):

| Group | Name | Class | Cards |
| --- | --- | --- | --- |
| myContacts | Contacts | seed | 33 |
| 1da344718a775160 | Seed amis | seed | 2 |
| 25150338fd4b2a9 | carillon-live-1794595616860 | CI | 0 |

Calendars:

| Calendar | Class | Items |
| --- | --- | --- |
| google@pimalaya.org (primary) | seed | 66 |
| Seed projets | seed | 14 |
| Holidays in France | system (subscribed, read-only) | 209 |
| neverest-live-1794743288720 | CI, deleted while being counted | n/a |

## microsoft (microsoft@pimalaya.onmicrosoft.com)

Mail (Graph folders):

| Folder | Class | Total | Unread |
| --- | --- | --- | --- |
| Inbox | seed | 1321 | 99 |
| Archive | seed | 1227 | 0 |
| Sent Items | seed | 445 | 0 |
| Deleted Items | seed | 835 | 5 |
| Clients | seed | 310 | 0 |
| Factures | seed | 263 | 0 |
| Projets | seed | 0 | 0 |
| Projets/Alpha | seed | 255 | 0 |
| Junk Email | seed | 1 | 1 |
| Drafts | system | 0 | 0 |
| Outbox | system | 0 | 0 |
| Conversation History | system | 0 | 0 |

Contacts:

| Folder | Class | Cards |
| --- | --- | --- |
| Contacts (default) | seed | 42 |
| Seed amis | seed | 8 |

Calendars:

| Calendar | Class | Items |
| --- | --- | --- |
| Calendar (default) | seed | 65 |
| Seed projets | seed | 15 |
| Birthdays | system | 64 |
| United States holidays | system | 90 |
| neverest-live-1793273597750 | CI | 0 |
| io-msgraph-test-1793784494490 | CI | 1 |
| io-msgraph-test-1794577590893 | CI | 1 |
| io-msgraph-test-1794587484799 | CI | 3 |

## Reading it later

The mail rows a CI suite can reach without making its own collection (a message a suite sends the account itself lands in INBOX or Inbox, and its copy in SENT or Sent Items) may drift by a few messages between two counts. A difference there is read against the CI runs of the interval (`gh run list`) before it is called a loss. Every other seed row must come back exact.
