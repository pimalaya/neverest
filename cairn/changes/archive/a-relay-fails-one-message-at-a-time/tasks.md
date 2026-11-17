---
cairn: tasks
change: a-relay-fails-one-message-at-a-time
---

# Tasks

- [x] Tests over two fake IMAP servers: a target refusing a 600,000-octet append at its command line returns (guard: passes on the old logic since 8c28411); a body longer than listed is refused and the same sides relay again; a refused message is rejected and the next relayed. The last two fail on the old logic.
- [x] Fake IMAP: `refuse_next_append_line`, `keep_appends` (APPENDUID).
- [x] `Exact` sink, recovery of both sides, per-message rejection in `relay_copies`.
- [x] Spec, log.
