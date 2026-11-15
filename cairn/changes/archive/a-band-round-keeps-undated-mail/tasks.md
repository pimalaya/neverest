---
cairn: tasks
change: a-band-round-keeps-undated-mail
---

# Tasks

- [x] Pin io-pimdir `ff28408` (Cargo.toml, Cargo.lock).
- [x] Remove `Held::undated`, `Bound::undated` and the Graph band page's relisting of bound undated messages.
- [x] Keep `widenings_list_their_band_and_keep_the_link` (the undated message stays through three widenings, now without the workaround; fails on io-pimdir `f9b13f8`).
- [x] Add `a_band_round_keeps_the_undated_mail_it_does_not_list` (src/offline/driver.rs): an IMAP or Gmail band round missing an undated and a garbled message keeps both (fails on `f9b13f8`). tests/scope.rs (Stalwart) still widens with its undated message.
- [x] Spec folded, log written, CHANGELOG.
