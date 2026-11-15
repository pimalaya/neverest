---
cairn: change
id: bodies-follow-their-page
status: landed
created: 2026-10-07
---

# A landed page's bodies download while the next pages list

## Why

`scoped-mail-sync` made a mail round land page by page, so the first headers of MOA's Microsoft 365 test box (Inbox 1,254 mails, 451 MB) were readable after 5 to 14 s instead of 18 to 30 s. The bodies did not follow: the one-source sync hydrated only in phase 2, once every collection's listing had ended, so the first body was readable after 17 to 37 s and the Inbox whole after 74 to 110 s. MOA's triage and replies read bodies, newest first.

## What

- **A pager on the sync verb.** `run_verb_paged` tells a `Pager` each time the coroutine asks for a page: the store is then at rest (the page enumerated last has been written whole, and the coroutine reloads what the next page names), and once more when the verb completes, for the last page. It releases the collection while the listing waits on the server and takes it back before the page is merged.
- **Phase 1 feeds, idle connections drain.** Each collection's spine holds a gate on its collection, released only during those waits. At each landed page its bodiless members go to a feed, in batches of 64 in the download order; before the first page of a round an earlier run left open, every bodiless member of the collection does. A worker with no collection left to spine takes the feed's next batch in the download order across the account, fetches it on its own connection, then applies it under the collection's gate (and one apply lock), so a body never lands between a page's load and its write. Workers leave phase 1 once every spine has ended and the feed is empty.
- **Phase 2 reads what is left.** The bodies phase 2 downloads are read from the store after phase 1, so none is fetched twice. A run over one connection has no idle one: everything waits for phase 2 as before, ordered across the whole account. A batch that fails to download or apply in phase 1 is warned about and left for phase 2.
- **The pull plan stays whole.** A body the feed kept before the pull plan is read is still this run's fetch: the plan names what was bodiless before the pull, not only what the pull added.
- **io-pimdir `f9b13f8`**: an empty mail date is stored as no date.

## Not done

- One connection does not interleave listing and bodies: with `connections = 1` the run behaves as before.
- Listing and hydration share the store's single writer, so a round closes a little later than before (1.5 s instead of 1.24 s on the local measure below) while its bodies land much earlier.
