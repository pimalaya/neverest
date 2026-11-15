---
cairn: tasks
change: a-gmail-round-lists-its-head
---

# Tasks

- [x] `client`: `Client::heads` answers delta heads for Gmail; `Listed::HeadRejected` constructed.
- [x] `gmail/client`: `enumerate` answers a delta head from the history since the round's history id, `HeadRejected` on a 404 or no history id, `NoHead` for a band head.
- [x] `gmail/run`: what each label's head touched, and when it began (`headed`, `head`).
- [x] `gmail/client`: a round page reads a touched message no earlier than the head, leaves out a message read without the label, reads flag listings again after a head.
- [x] `offline/driver`: `sync_side` passes the source's heads to the sync verb; `offline`: a head starts a resumed round for the pager.
- [x] Tests over scripted Gmail sessions: a head lists arrivals, deletions and the new history id; an expired head is rejected and a band head answered none; a page after a head reads what it touched anew and leaves out what left the label; flag listings read again; end to end through the driver: an arrival lands before the next page and survives the close, the round closing on the head's history id (vector 62); a deletion in the head drops the member at once; an expired head restarts the round.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy -D warnings` (default, `--all-features`, MOA's set), `cargo fmt --check`.
- [ ] A live run on the test account.
