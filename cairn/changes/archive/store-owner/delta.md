---
cairn: change
change: store-owner
---

# Delta

## ADDED Requirements

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and
enqueue mutations through io-pimdir's producer queue. At the start of every sync
run, before any network work, each collection with pending queue work SHALL be
drained (`drain_collection`: exactly-once apply-and-delete per action,
permanently bad actions parked, transient failures left queued in order). The
applied counts SHALL be logged (info when nonzero) and reported, and every
parked action SHALL surface in the run report until repaired. The subsequent
sync of a drained collection pushes the resulting dirty state; a drained
local-only collection is flushed but never remote-synced.

### Requirement: A run holds the store lock, waiting bounded
A sync run SHALL hold an advisory `sync.lock` in the **actual** store directory
(honouring `store.root`) for the whole run. A second run SHALL wait for the
holder up to a bounded timeout (60 s) and then exit with a clear error, so cron
ticks and connector-triggered scoped runs serialize instead of failing or
corrupting.

### Requirement: An IMAP handle-space change rebuilds the collection and bumps its generation
For an IMAP side, the driver SHALL compare the stored checkpoint's UIDVALIDITY
before and after the pull; on a change it SHALL drive io-replica's rekey
(carrying cached bodies, summaries and pending state over by link id) and route
the rebuild write batch through `write_rekeyed`, so `collections.generation`
bumps atomically with the rebuild and a frontend derives its epoch (an IMAP
UIDVALIDITY) from the store alone. Ordinary syncs and full resyncs never bump.
Graph sides never rebuild: Graph message ids survive a delta reset (an expired
delta link restarts a full round without changing identity).

### Requirement: A two-source sync may mirror every body
`store.hydration = "full"` SHALL make a two-source retain sync hydrate every
non-tombstone placement to `Full` on both sides (bodies mirrored in the store),
reusing the body dedup so a shared body is fetched once. The default stays
per-mode: a one-source sync always retains every body, a two-source sync
hydrates only bodies about to cross (`"crossing"`). Full hydration forces
retain; combined with an explicit `relay` it warns and retains.

### Requirement: Microsoft Graph is a first-class side
An `msgraph` side SHALL open protocol-direct over io-msgraph (never through a
frozen aggregator): folders listed two levels deep (`Parent/Child` naming),
enumeration through the messages delta query carrying the `@odata.deltaLink`
as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh
full round; any other failure surfaces), the `Meta` tier served from the cached
delta rows (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME
content streamed into the blob store. Flags map to the IANA wire spellings
(`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`).
Auth SHALL support a plain bearer token plus three OAuth flows over io-oauth:
device-code (refresh token persisted at `<store>/tokens.json` mode 600, silent
renewal), client credentials by secret, and client credentials by RFC 7523
certificate assertion; no token is ever logged. Push scope is honest: flag
changes push through `message_update` and deletes through `message_delete`;
appends, moves and content updates are rejected (pull-only) and documented.

### Requirement: The Outbox is local-only and flushes through the send channel
The `Outbox` collection SHALL never be enumerated against a remote. After the
queue drain, every placement staged as a creation in it SHALL be sent through
the account's send channel — SMTP submission when the account configures an
`smtp` table (`server` `smtps://…:465` or `smtp://…:587` + `starttls`, optional
login/password), the Graph `sendMail` action for a Graph-backed account (which
saves to Sent itself) — and dropped from the store on success. The SMTP
envelope comes from the placement's outbox meta (`v`, `from`, `rcpts`, …); a
per-message failure leaves the placement queued for the next run and never
kills the run. Message content is never logged.

### Requirement: The checkpoint is opaque to the shared client seam
The backend-neutral enumeration seam SHALL carry the incremental-sync cursor as
opaque checkpoint bytes and string member handles: the IMAP adapter encodes its
`(UIDVALIDITY, HIGHESTMODSEQ)` pair, the Graph adapter its delta link, and the
engine stores whichever bytes the side produced. (Supersedes the IMAP-shaped
`(u32, u64)` cursor on the shared seam; the QRESYNC behaviour itself is
unchanged and stays specified under "IMAP enumeration is incremental".)

## MODIFIED Requirements

## REMOVED Requirements
