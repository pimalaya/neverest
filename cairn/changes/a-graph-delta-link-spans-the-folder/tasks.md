---
cairn: tasks
change: a-graph-delta-link-spans-the-folder
---

# Tasks

- [x] `src/msgraph/client/mail.rs`: `MailWire`, band listing on `sentDateTime`, the pass, deltas reading the summaries of unbound members, tagged links and cursors.
- [x] `src/msgraph/client.rs`: generic `batch_get` for bodies and summaries (404 named gone), the old filtered delta removed.
- [x] `Client::scope_bound` false for Graph mail; `Held` carries the coverage and the bound undated messages (`src/client.rs`, `src/offline/remote.rs`).
- [x] Unit tests over a fake Graph folder and the real engine: exact band bounds, three widenings without relisting, out-of-scope changes dropped, summaries read for unbound members only, interruption in the band and in the pass, a whole-folder link serving later scopes, a filtered link of before refused.
- [x] tests/msgraph.rs: `a_graph_scope_widens_by_its_band` (ignored, live).
- [x] Spec folded, log written, CHANGELOG.
