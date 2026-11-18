---
cairn: log
change: a-pulled-change-reads-as-one
date: 2026-11-18
---

# A pulled change reads as one

Line R1 of the N9 deletes report of 2026-11-15, seen again in the live run of 2026-11-17, ruled by the owner on 2026-11-18 before v1 freezes the report: a removal the server made and the store took in read `{"kind":"delete","side":"imap"}`, exactly as a delete the run pushed to that server; pulled flag changes read as pushed ones the same way.

## What landed

- src/sync/hunk.rs: `Direction` (`push`, `pull`), a `direction` field on `AddFlags`, `RemoveFlags` and `Delete`. A pulled one prints `pull <flags> added to item <id> in <collection> from <side>`, `pull <flags> removed from ...` or `pull the deletion of item <id> in <collection> from <side>`; a pushed one prints as before.
- src/sync/driver/itemize.rs: `itemize_pulled` builds `Pull` hunks, `placement_hunks` `Push` ones; a rejected write names pushed hunks alone.
- Tests: `a_server_delete_is_named_once_on_each_endpoint` checks the pull on the endpoint the server dropped the card from and the push on the other, in JSON and in text (fails to build on the code before); tests/multikind.rs (Stalwart and Radicale, not run here) checks its server-made flag change and removal read `pull`.

## Spec

- [sync](../spec/sync.md): *The report shows remote-originated changes a pull applied* names the direction and the text.
