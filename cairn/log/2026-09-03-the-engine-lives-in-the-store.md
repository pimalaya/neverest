---
cairn: log
change: the-engine-lives-in-the-store
landed: 2026-09-03
---

# The engine lives in the store

io-replica folded into io-pimdir on 2026-09-03, and the store caught up with the pimdir standard of the same day. neverest was the last consumer of the retired crate, and three things it carried because the crate below fell short have gone to that crate.

## What moved down

**The storage seam.** The `ReplicaStorage` trait is gone; `PimdirSourceStore` services its own loads, lookups and writes, and a foreign driver hands it each storage yield through `service`. neverest keeps its driver, which times every yield kind for `NEVEREST_PROFILE` and serves the `Full` apply from the pre-fetch cache, and it now does exactly that and nothing else. The rekey pump went with the trait: a rebuild's batch drops every old handle as `Rekeyed`, the store bumps the generation in the transaction applying it, and the driver reads the number back.

**The identity narrowing.** `HeldStore` filtered a source's load to the identities it binds, because the projection carries the copy the hub offers for an item the source lacks and the upgrade read that offer as a claim on the identity, minting `dup:` for the resource the second endpoint had always held. That is the mint's own condition in the pimdir spec (SYNC §6, minted when *this source* binds the hint under another handle), so it landed in io-pimdir's store: a load by key answers with the rows the source binds. Its regression test is `a_second_source_holding_an_offered_item_binds_it_rather_than_minting` in io-pimdir's engine suite, red without the fix and green with it.

**The derivations.** `kind::vcard` and `kind::ical` are deleted and `kind::mail` no longer reads headers: `summary::derive` decodes RFC 2047 words, splits a vCard property on the first colon outside a quoted parameter and unescapes its value, and resolves a calendar start through the resource's `VTIMEZONE`. What stays here is the `Meta` tier, `PimdirMailSummary` built from the envelope, and the two adjustments the streamed tier owes: the octet length the stream knows and the prefix does not, and an attachment left unknown since no part was walked. The envelope now carries `Cc` and `Bcc`, so the address rows the two tiers write agree.

**The summary.** `meta` was a JSON blob per kind; it is the five summary tables and `item_address`, typed. The queue's `update` carries no summary any more, the owner deriving it from the body, so the run's own merge and `conflict resolve` stage the hash alone.

## What stayed

`Kind::split_link_id`, the read side of a minted key, stays: the engine mints `dup:<hint>#<handle>` and hands a write the whole key, and a DAV resource name has to carry the mint or the second `PUT` lands on the first copy. `Kind::merge` stays, the engine knowing nothing about merging formats. The report, the phases, the relay and the conflict commands are untouched.

## Verification

149 unit tests, every live suite against the local Radicale and the two Stalwarts (19 tests, one binary at a time), `cargo clippy --all-targets` clean, `cargo fmt`. io-pimdir with the one edit: its full suite green (`--all-features`) and clippy clean.

A pull-only run against the Fastmail account from a fresh store, `one-way = true`, no target, every write permission off: `init`, a first `sync` fetching 24 items across the calendar, the address book and seven mailboxes, a second `sync` already in sync. `pimdir store info`, `collection list`, `item list`, `item show`, `check` and `queue list` read the store back consistent, with typed summaries and one minted key in `imap/INBOX`, and `conflict list` holds nothing.

## Capabilities moved

- sync: the summary is the format's typed row, the derivations are the format's, link id and summary per kind, the sort key, the rekey bump, the checkpoint and change names, the held delete, one identity across endpoints
