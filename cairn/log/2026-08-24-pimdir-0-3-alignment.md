---
cairn: log
change: pimdir-0-3-alignment
landed: 2026-08-24
---

# Realign on io-replica 0.4, io-pimdir 0.3 and the io-sasl split

The libraries moved and neverest no longer built: SASL left pimalaya-stream for io-sasl, io-imap 0.6 and io-smtp 0.3 put their commands behind client traits with a session-options struct, io-webdav 0.2 renamed its CardDAV types, io-replica 0.4 turned a flag set into `Unknown | Known` and gave a placement a sort key, and io-pimdir 0.3 made the store own its content hash. Fifty-odd compile errors, two of them standing for something real.

## What landed

**The sort key.** A kind now derives three things rather than two, and the third rides on every fetched item: the `Date:` header in RFC 3339 UTC at seconds precision for mail, the casefolded `FN` for a card (pimdir SPEC Annex A). Mail derives it at both tiers, through the RFC 3339 string both already put in the meta, so the key cannot move when a body arrives, which is the hazard the `alt:` link id already taught this file.

**The hash.** `offline::hash` is gone. It computed a 128-bit FNV-1a rendered as hex while the store recorded `blake3` and the Android app wrote base32 `sha256-128`: three implementations of one store naming the same body three ways, no dedup and no blob found, silently. `HydrateSink` now folds the hasher the store hands out, so a body is named the way `store_meta.hash_algo` says it is.

**The account.** Every store handle opens `for_account`, so a store two hand-written accounts share (a pairing the wizard refuses but a config can express) says whose collection is whose instead of leaving a reader to guess from the naming.

**SASL.** Credentials come from io-sasl, with the SCRAM nonce left empty for io-imap to draw at connect (an I/O-free coroutine cannot generate randomness). `io-imap/scram` is enabled, since the config has always offered SCRAM-SHA-256 and the mechanism would otherwise be refused at authentication. The wizard offers only the six mechanisms `SaslConfig` can spell: io-sasl names every registered one, and a probe that returns another now drops it rather than reaching an unreachable arm.

## What the live run found

Two bugs the CardDAV backend had all along, both of which made it unusable and neither of which any unit test could have caught. They surfaced running `tests/carddav.rs` against a real Radicale.

**Everything after the first request failed.** Radicale's built-in server answers HTTP/1.0 and closes the connection per response; io-webdav holds one stream, reports no keep-alive hint and never reconnects, so the second request was written into a socket the peer had hung up on. The CardDAV client now reopens and runs the exchange again on an end-of-stream or reset failure, carrying the discovered principal and home-set URLs over. The Graph backend already did this on the hint its client reports; the proper fix is io-webdav surfacing the same hint, and then this retry becomes the fallback rather than the mechanism.

**Every DAV collection failed its scan.** The driver raised freshly probed items to `Meta` unconditionally, which asks a CardDAV side for the summary tier the spec already says it does not have. The tier is now the kind's (`Kind::probe_tier`), `Full` for DAV, and `upgrade_meta` is renamed `upgrade_probed` for saying what it does.

With both fixed, a first sync lands both cards keyed by their `UID`, with blake3 base32 object names, known-empty flags, a `v:1` meta and a casefolded sort key, and a server-side delete is retained with its body rather than lost.

## Left open, in io-replica

A server-side **edit** still leaves the item bodiless. The sync drops the stale body and lowers the placement to `Probed` (io-replica sync.rs), but `ReplicaHub::absorb` merges the level as `max`, so the item stays `Full` with no object, and `ReplicaUpgrade` skips whatever already reads as `Full`. Nothing refetches it, and the pass that would have does not run.

The hydration pass now keys on the missing body rather than the level, which is the correct question to ask and makes the intent visible, so the moment io-replica stops raising a bodiless item to `Full` the refetch lands. Until then a refreshed card re-downloads on every run and the store keeps the old summary. Invisible to mail, whose bodies are immutable and whose revision is always `None`.

## Capabilities moved

- **sync**: gained the per-kind sort key, the store-owned hash, the account grouping, the kind's probe tier, the DAV reconnect and the remedy named on a store the format outgrew; hydration now selects on the absent body rather than on the detail level.
