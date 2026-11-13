---
cairn: log
change: a-capped-source-says-so
date: 2026-11-13
---

# A capped source says so

pimdir audit line Y4. A source whose server lists at most a number of what a collection holds is declared in the configuration, every page of its rounds states `capped` (pimdir SYNC §4, §5), and the report counts what a round left unlisted.

## What landed

- **The key**: `<backend>.collection.capped`, a boolean, false by default (src/config.rs, `CollectionSourceConfig`).
  - Under `collection` because that table already holds what a source states of its collections (`filter`, `create`, `delete`) and exists on every backend, so the key needs no table and no per-backend code.
  - `capped` because it is the standard's word and the page field's.
  - A boolean because neverest needs no number: the engine counts what a round left unlisted, and a limit compared with what a scoped round lists would prove nothing.
  - Per source because the limit the audit names (Gmail's "Limit IMAP folders to contain no more than this many messages") is one setting for every folder.
- **The listing**: `SourceCtx::capped` carries the key, `PimRemote::with_capped` takes it on the two remotes that list (the sync pass and the rekey, src/offline/driver.rs), and `PimRemote::enumerate` states `capped` on every page that belongs to a round and on no other (src/offline/remote.rs): a delta and a delta head are no round's page, and a band head is answered before.
- **The report** (src/sync/report.rs): `unlisted`, one `{side, collection, count}` per collection a capped round of the run closed on, the most one pass counted (`PimdirSyncReport::unlisted`, through `Noted`). Printed under `Unlisted`, as `10 item(s) of INBOX not listed by imap, which is capped: kept`. It is no warning and changes no exit code, as `waiting` is none: a capped source leaves the same part out at every round, and a warning every run would be noise. The JSON schema of `neverest-sync` follows from the type.
- config.sample.toml (the key, commented, with what it costs, and a line in the per-source overview), README and CHANGELOG.

## Tests

- src/offline/driver.rs, `a_capped_source_keeps_what_its_listing_leaves_out`: 25 messages bound, a fake IMAP server without QRESYNC listing the 15 newest. Not capped, the round drops the 10 (15 held, nothing reported). With `imap.collection.capped = true` read from a configuration, the run reports `unlisted` 10, parks nothing, holds 25, and the report's JSON and text name them with no `Warnings` block. Seen failing with the seam stating `capped: false` as it did (the assignment forced false by hand for one run): `assertion left == right failed, left: 0, right: 10` on the engine's count, the 10 dropped.
- The sample's new lines are comments: `the_documented_sample_still_loads` passes as before and does not exercise the key, which the test above reads from a configuration.

## Evidence

As the log of a-message-is-keyed-by-the-formats-reading states, the three changes verified together: 508 unit tests, every clippy set clean.

## Not done

- **A rekey of a capped collection is not protected by the cap.** pimdir SYNC §8 states no rule for a capped listing and io-pimdir 0.11 reads `capped` in a sync alone, so a rekey (a moved `UIDVALIDITY`) drops every binding its listing does not resolve unless its own park stops it: 900 of 1,900 is under the half. neverest states `capped` on the rekey's pages all the same, for an engine that reads it. The standard's, then the engine's.
- **A capped source on a server stating no removal keeps what was deleted there**: IMAP without QRESYNC, Gmail's among them, states none, so a message deleted on the server stays in the store while the key is set. The sample, the README and the CHANGELOG say so. The standard's rule, not a choice of this change.
- No server states its cap, so nothing is detected: a user who sets Gmail's limit and not the key still loses the 900 from the store, as before.
- A number-shaped key (the limit itself), left out on purpose: the owner's call if a later connector can use one.
- No server test over Gmail's IMAP with the limit set.

## Capabilities moved

- sync (cairn/spec/sync.md): added "A capped source says so, and keeps what it leaves unlisted".
