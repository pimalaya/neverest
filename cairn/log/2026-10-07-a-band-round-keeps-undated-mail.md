---
cairn: log
change: a-band-round-keeps-undated-mail
date: 2026-10-07
---

# A band round keeps the undated mail

io-pimdir `ff28408` (pimdir `00535c1`) makes a band round infer no delete of an undated member, the bug `a-graph-delta-link-spans-the-folder` (3938d10) had worked around for Graph alone.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir pinned at `ff28408a7834cc43cdd6e7229510174a783356fd`. neverest builds no `OpenRound` and reads no `PimdirRound`, so nothing else adapts.
- src/client.rs, src/offline/remote.rs, src/msgraph/client/mail.rs: `Held::undated`, `Bound::undated` and the relisting of bound undated messages on the last page of a Graph band round removed; `band_page` no longer takes `Held`.
- Tests: `widenings_list_their_band_and_keep_the_link` (src/msgraph/client/mail.rs) passes without the workaround and fails on `f9b13f8`. New `a_band_round_keeps_the_undated_mail_it_does_not_list` (src/offline/driver.rs): a source bound to no scope (IMAP, Gmail) widens, its band listing neither a message with no `Date` nor one with a garbled `Date`, and both stay; it fails on `f9b13f8`. `PagedRemote` gives no checkpoint on a band round. tests/scope.rs (Stalwart) still widens with its undated message, passing.

## Capabilities moved

- sync: a mail sync lists within a scope on the `Date` (modified: a band round infers no delete of undated mail, scenario added); a Graph mail folder keeps one delta link (modified: the band's last page no longer relists undated messages).
