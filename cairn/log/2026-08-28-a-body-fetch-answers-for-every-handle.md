---
cairn: log
change: a-body-fetch-answers-for-every-handle
landed: 2026-08-28
---

# A body fetch answers for every handle, or says so

Found while chasing a CardDAV account whose every card came back empty. The cause turned out to be entirely server-side (a Posteo app password authenticates and lists an address book, but the cards it serves are zero bytes), and neither of these defects caused it. Both are how neverest turned that into an unreadable failure mode instead of a message.

**A short batch counted as a success.** `hydrate_batch` sent 64 handles to `addressbook-multiget`; io-webdav drops any entry whose `address-data` is empty, so 2 items came back for 64 handles and were returned as the batch's result. The engine recorded 2 and heard nothing about the other 62, which it cannot distinguish from handles it never asked about, so it asked again on the next run, and the next. Every run enumerated 153 members, fetched them all, stored nothing, and printed "already in sync"; a dry run beside it counted 152 hunks. The per-item fallback that would have caught this existed already, but only fired on a batch *error*, and a batch that answers for a subset does not error. It now fires on a shortfall too, naming the count.

**An empty body was stored as an item.** A zero-byte body hashes to the digest of nothing, `hash:cbf29ce484222325`, so every empty card resolved to the same link id. The second arrival collided with the first, the `duplicate-link-id-freeze` floor did its job and froze the identity, and the collection stayed frozen for every later run: 9 handles piled into one binding's `ambiguous_handles`. A zero-length body is now refused with the item named. No kind here has an empty body, and refusing one turns a silent poisoning into one line on the first run.

Capabilities moved: sync, two new requirements on the fetch.
