---
cairn: log
change: a-relocation-lands-on-the-flags-it-carries
landed: 2026-11-24
---

# A relocation lands on the flags it carries

Found by the live test night of 2026-11-23 (.lanes/live/report.md of the workspace, finding 1.1), a silent loss: `himalaya flag set -f seen -f important`, then `message move` of the same message through a pimdir account, drained by `neverest sync` on Stalwart. The move pushed, then the next sync pulled the server's `\Flagged` back over the queued set, on the server and in the store, exit 0.

The cause was io-pimdir's: the landed create took the staged flags as its base while `UID MOVE` kept the server's. io-pimdir 65a5026 bases it on the flags the member carries (pimdir d5645bf, SYNC §4, vector 214). neverest only moves its pin from 17d4ea8 to 65a5026; its spec has no rule of its own on the landing.

## Checked

- Suites on the new pin: 742 passed, 0 failed.
- Live on Stalwart (stores/pimdir-stalwart): a `\Flagged` message, `flag set -f seen -f important` then a move queued through himalaya's `stalwart-sep`; the drain pushed the move and the set, the server answered `(\Important \Seen)`, the store agreed, and two more syncs said "already in sync".
