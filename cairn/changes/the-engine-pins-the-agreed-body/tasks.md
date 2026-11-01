---
cairn: tasks
change: the-engine-pins-the-agreed-body
---

# Tasks

- [x] Wait for io-pimdir pinning the agreed body (pimdir spec edit 4); pin it (the local checkout, as for Phase A).
- [x] Remove `shared_bodies`, `diverged_items`, `park_divergences` (and `hub_divergences`, `hub_conflicts`, `park_hub_conflicts`); keep their tests green.
- [x] Reduce the raw `UpsertPlacement` writes and `load_hub` reads to what io-pimdir offers no verb for (`bind_relayed`; the relay targets, projection, hydration targets and reset reads).
- [x] Settle when `items.conflicted` clears and read it accordingly (the engine's, on an edit or a remove; no sync reads it).
- [x] Tests: `a_card_both_endpoints_rewrote_diverges_against_their_agreed_body` and `a_card_two_endpoints_never_agreed_on_parks_with_no_ancestor`, the two shapes the workaround handled, over the engine alone; the Radicale suite (tests/endpoints.rs) is ignored by default and was not run.
- [x] Spec folded, log written, CHANGELOG.
