---
cairn: change
id: a-band-round-keeps-undated-mail
status: landed
created: 2026-10-07
---

# A band round keeps the undated mail

## Why

A widening of the scope lists only the band its coverage lacks, by the provider's date filter: IMAP `SENTSINCE`, Gmail's `after:` and `before:` on the arrival, Graph's `sentDateTime`. None of them surely lists a message with no usable `Date`: IMAP lists the ones with no `Date` header but not a garbled one, Gmail lists one only if it arrived in the band, Graph never. Yet io-pimdir inferred the deletion of every bound undated member a band round did not list, so each widening dropped them until a whole-scope round relisted them. `a-graph-delta-link-spans-the-folder` worked around it for Graph alone, relisting the bound undated messages on the last page of a band round (`Held::undated`).

## What

- Bump io-pimdir to `ff28408` (pimdir `00535c1`): `PimdirWriteOp::OpenRound` and `PimdirRound` carry `band` (`sources.round_band`), a band round infers no delete of an undated member, and an open round of the other kind restarts. neverest builds neither, so only the pin moves.
- Remove the Graph workaround: `Held::undated`, `Bound::undated` and the relisting on the last band page.
- Test the engine's behaviour for every source bound to no scope: a widening whose band lists neither an undated nor a garbled message keeps both.
