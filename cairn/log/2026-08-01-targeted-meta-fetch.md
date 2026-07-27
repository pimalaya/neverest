---
cairn: log
change: targeted-meta-fetch
landed: 2026-08-01
---

# Targeted Meta / size fetches (kill the silent whole-mailbox scans)

Syncing a large mailbox was slow in the silent phases bracketing the download
`%`, even when little changed. Two whole-mailbox `FETCH 1:* (… ENVELOPE …)`
sweeps ran per changed mailbox, reporting nothing: `fetch_meta` listed the entire
mailbox's envelopes to resolve a handful of link ids, and `sizes()` listed the
entire mailbox *again* just to read `RFC822.SIZE` (fetching the full ENVELOPE it
never used). `enumerate` was already lean and targeted; `Meta` and `sizes` were
not.

Both now target only the handles being processed, mirroring `enumerate`:

- New IMAP backend `fetch_envelopes(uids)` → `UID FETCH <set> (UID FLAGS ENVELOPE
  RFC822.SIZE)`; `fetch_meta` uses it.
- New IMAP backend `fetch_sizes(uids)` → `UID FETCH <set> (UID RFC822.SIZE)`
  (size-only, no ENVELOPE); `sizes()` uses it.
- Both exposed on `Client`. The now-dead whole-mailbox `list_envelopes` (and its
  `compute_window` helper) were removed — the sync engine never lists whole
  mailboxes anymore.

Incremental syncs drop from O(mailbox) to O(changed); a first sync stays heavy
(each new message's link id is fetched once) but the redundant second sweep is
gone.

Verified live against Stalwart: an incremental sync of a single new message issues
`UID FETCH 7 (UID FLAGS ENVELOPE RFC822.SIZE)`, then `UID FETCH 7 (UID
RFC822.SIZE)`, then `UID FETCH 7 (BODY.PEEK[])` — three single-UID fetches where
before the first two were `1:*` sweeps. First sync downloads all bodies, no-change
sync stays "already in sync" (QRESYNC, zero fetches), dry run lists the pull plan.
14 unit tests green, fmt/clippy clean (only the pre-existing autoconfig warning),
relay integration unregressed.

Spec updated: `sync` (ADDED: Meta and size fetches are targeted).

Aside (not in this change): `sync`'s init-check reads `replica_dir` while `init`
writes to `store_dir`, so a configured `store.root` makes `sync` report the
account uninitialized. Latent bug, noted for a follow-up.
