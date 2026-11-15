---
cairn: log
change: the-alignment-housekeeping
date: 2026-11-15
---

# The alignment housekeeping

Ledger lines N29, X2, X6 and W8 (held) of the CLI alignment round of 2026-11-15.

## What landed

- tests/msgraph.rs `a_graph_add_lands_in_drafts_only` expects the add outside Drafts refused at enqueue (`PimdirError::Unsupported`, `mail.message.add`, the Drafts-only detail), as `drafts_only` declares since efc2cb3; tests/common/mod.rs `Replica::try_add` answers the refusal.
- .github/workflows/tests.yml: one live job per provider (`live (google)`, `live (microsoft)`, `fail-fast: false`), so Google's quota no longer stops the Graph suite.
- MIGRATION.md headings read "From vX to vY", content unchanged.
- The 113 landed changes left in cairn/changes/ moved to cairn/changes/archive/; the five active ones and the jmap-sources draft stay.
- cairn/verify.sh, calendula's. Conformant but for cairn/spec/testing/, the live-testing session's.
- pimalaya-config 0.5.2.

neverest stays on io-webdav 0.9.1 until io-webdav exposes the served href of a listed entry, so a key can be the segment served: 0.10 decodes listed ids, and re-encoding them cannot restore a segment a server sent with a raw space or non-ASCII byte, which would change a store key.

No code table was built: every code is specified in cairn/spec/sync.md under the requirement raising it (see the change's decisions).

No capability moved.
