---
cairn: change
id: a-download-order-is-chosen
status: landed
created: 2026-10-04
---

# A download order is chosen

## Why

The one-source sync downloads bodies largest first across the account. That is the fastest order for a whole run: the big bodies start early and the pool ends on small ones. But a caller showing mail while the first sync runs wants the opposite on a large box: today's mails, the ones a reader or a triage wants, come last, after years of large attachments. Reported by MOA (`docs/plan/first-sync.md`, "A very large Inbox"): its staged first sync makes the Inbox readable first, yet within the Inbox the newest mails still wait for the largest.

Neither order suits every caller, so the caller chooses.

## What (design)

- **`sync --download-order <largest|newest>`**, `largest` by default: today's behaviour is unchanged.
- **`newest`** sorts each collection's bodies by their date, newest first, chunks them in batches as before (a batch stays in one collection), and queues the batches across the account by their newest body. Items without a date (no `Date` header, or a non-mail kind, which carries none) come last, in the order they were planned.
- The date is the store's own: the mail summary's RFC 3339 instant in UTC, read from the headers at the `Meta` tier. No server request is added.
- The same comparison orders the items of a collection and the batches across collections, a batch standing for its first item.

## Out of scope

- An account configuration key: a CLI flag is enough for the caller that asked; one can follow, as `connections` has.
- The two-sided sync's hydration, which has no sizes either and keeps UID order.
