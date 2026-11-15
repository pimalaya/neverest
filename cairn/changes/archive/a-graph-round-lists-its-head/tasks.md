---
cairn: tasks
change: a-graph-round-lists-its-head
---

# Tasks

- [x] `client`: `Client::heads` answers band heads for Graph mail.
- [x] `msgraph/client/mail`: a band head listed by `receivedDateTime` from the round's start less two days, members bound by their flags, others by their summary; a delta head and an unreadable instant answered none.
- [x] `msgraph/client/mail`: the last page of a whole-folder round and of a band round lists the head again after its own rows; the pass reads a member the head named by its summary; the instant dropped by a fresh round, the pass and the last page.
- [x] `msgraph/client`: the listing by arrival with the size `$expand` and its 400 fallback (`received_first`); a scripted session for driver tests.
- [x] Tests: the instant's forms and the margin; a whole-folder round, a scoped round and a band round resumed over the fake folder (arrival landed before the next page, kept by the last page, one gone before it found absent), each checked by a mutation; the `$expand` fallback on the head; end to end through the driver over a scripted Graph (vectors 65 and 66).
- [x] `offline::driver::graph_budget::a_throttle_on_one_connection_stops_every_connection` made deterministic.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy -D warnings` (default, `--all-features`, MOA's set), `cargo fmt --check`.
- [ ] A live run on a Microsoft 365 test tenant.
