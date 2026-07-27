---
cairn: log
change: generic-pim-sync
landed: 2026-08-07
---

# Neverest syncs a second kind: contacts over CardDAV

The change that turned a mail sync into a generic PIM sync is complete except
for CalDAV, which moves to its own change. Phases 1 to 3 (vocabulary, the kind
seam, mutable content) landed earlier; this entry covers phase 4 (CardDAV) and
phase 6 (wizard, docs, spec), and the point of them together: everything phase 3
built was written against a fake remote, because mail structurally cannot
exercise it. Mail bodies are immutable, so revisions, conditional writes and
content conflicts had never met a server. Now one kind does.

**The kind** (`kind/vcard.rs`): a card's link id is its vCard `UID` (`uid:`),
falling back to an FNV-1a digest of the body (`hash:`) when it carries none, and
its `v:1` summary carries the `UID`, `FN` and every `EMAIL`. FNV-1a rather than
`DefaultHasher` because its output is fixed by its own specification: a hash
that moved with a compiler version would silently re-link every such card and
store its body twice. `parse_summary` is `None` for cards, so they resolve at
`Full` only, which is why they cannot hit the two-derivations bug mail hit.

Cards are read by a small scanner (unfold, strip group prefixes and parameters,
split at the first unquoted colon) rather than a vCard parser. The sync needs a
`UID` and a few display fields and must never rewrite a card: bodies cross
neverest byte for byte, so a property the scanner does not understand cannot be
lost. That also keeps vcard-rs out of the release chain. Rendering a card is the
frontend's job, and that is where a real parser belongs.

**The backend** (`carddav/client.rs`): address books are collections keyed by
their path segment, not their display name (optional, mutable, free to collide).
Enumeration is RFC 6578 `sync-collection` throughout, the token riding as the
opaque checkpoint: a tokenless report returns the whole member set (so the
initial sync needs no separate `PROPFIND`), a rejected token falls back to one,
and a truncated report is drained, bounded. Bodies come from
`addressbook-multiget`; writes are conditional on the last-synced ETag; a move
is create-then-delete, the delete running only once the create was accepted, so
a failure leaves the card where it was rather than nowhere. Flags are
known-empty, never unknown. A new card is addressed by its `UID` sanitised into
one path segment.

**Configuration**: a `CarddavConfig` side (server URL, TLS, ALPN, Basic or
Bearer auth) through the same `side_config!` macro, so it inherits the
permission and pool fields. `AccountConfig::validate` refuses an `<side>.smtp`
table on a contacts side and runs before any connection: submission is a mail
capability, and a dead option is worse than an error.

**Wizard**: the discovery fan-out now asks for CardDAV too (io-pim-discovery
`rfc6764`, pulled in by the `carddav` feature), and a reachable endpoint is
offered beside the mail ones, ranked last. Picking it prompts a login and its
secret, then opens the session (which discovers the home set) as the connection
test. A run still writes one account; pairing a mail and a contacts account
against one `store.root` stays hand-written, and the spec says so rather than
promising the multi-account flow the delta originally drafted.

**Live suite** (`tests/carddav.rs`, `tests/radicale.sh`): ignored by default,
Radicale in Docker. It seeds two cards, syncs, edits one **on the server** and
checks the store followed, deletes one and checks it is retained rather than
lost, then syncs again and checks the run is quiescent, which is what proves a
retained row is invisible to the merge instead of re-derived every run. It
observes the store through the `pimdir` CLI, the tool for exactly that.

The `carddav` feature stays out of the default set until that suite runs in CI.

Verified: 68 unit tests green (7 new for the vCard kind, 3 for card addressing,
1 for the contacts account shape), fmt clean, clippy clean except the
pre-existing `incompatible_msrv` warning in `cli/sync.rs`.

Cross-repo: pimdir SPEC §13 gained the `text/vcard` `v:1` convention beside
`message/rfc822`, including the two facts that make it different (one
derivation, and mutable bodies whose revision moves while the link id does not).

Spec updated: `sync` (ADDED the seven kind, revision, conflict, permission, DAV
enumeration and known-empty-flag requirements; MODIFIED "Sides are remote
backends only", "Bodies are content-addressed and deduped", "A two-source sync
may relay instead of retain", "The wizard discovers in parallel and proposes
what it found", and the submit-intent requirement, which now states submission
is mail-only).

Deferred: **CalDAV** (phase 5) moves to its own change, on the shape this one
established. `kind::ical` will need one rule this change did not: a
`RECURRENCE-ID` must stay out of the link id, so a recurrence override remains
the same item.
