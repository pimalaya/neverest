---
cairn: log
change: named-sources
date: 2026-08-26
---

# An account holds named sources, not a left and a right

Setting up a Fastmail account, you pick a protocol for the right side and get IMAP. Then you want your contacts too. The engine could already do it: pimdir keys a collection by its media type and io-replica reconciles N sources against one hub. The configuration could not say it, because an account was a `left` and a `right` forced to agree on their kind.

## What landed

- **An account holds named sources** (capability `sync`). `sources.<name>.<protocol>` over one store, the map key being the pimdir source id. A backend written directly under the account (`imap.server = …`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account; the explicit table is what a mirror or a fan-in needs, since those need two sources of one protocol. The sugar and its expansion produce the same source id, so expanding one by hand is a no-op on disk, and that is load-bearing rather than tidy: a source id names every binding it owns, so a rename orphans them.

- **Collection namespaces decide whether two sources meet** (capability `sync`). A hub collection id is `<namespace>/<name>` with the kind on the collection row, and `PimRemote` strips the prefix back off at the one seam where an id reaches the wire, so no other code carries two spellings of a collection. Two sources sharing a namespace bind the same collections, and that sharing *is* propagation: an item in a collection a source participates in, with no binding for it, gets pushed. The namespace defaults to the source's own name, so sources are isolated by default. Isolated because its failure mode is a mirror that did nothing, visible in the report, where merging by default fails by copying one real provider's mailbox into another.

  A namespace shared by two kinds is refused. The id carries the namespace but not the kind, so a mailbox and an address book both called `Default` would key onto one collection; mirroring across kinds means nothing anyway.

- **What the store keeps is derived and reported, never configured** (capability `sync`). `store.retention` and `store.hydration` are gone. They encoded three states in two settings, one combination of which meant nothing, and the value they set is a consequence of how many sources share a namespace rather than a choice. One source keeps every body, two sharing a namespace on a streamable pairing keep none, anything else keeps what crossed. Every run and `check` report it, per kind and namespace, including on a run that wrote nothing, since the report is now the only place the value is stated.

  Deriving also deleted a silent substitution: an explicit `relay` on a DAV pairing used to fall back to retaining, handing back the opposite of the disk guarantee it asked for. With nothing explicit to honour there is nothing to substitute.

- **A derived change never drops what is stored**. The derivation moves when the configuration moves, and one of those transitions is destructive: adding a second source flips a namespace from keeping every body to keeping none. It governs only what a run fetches and keeps from then on. Stored objects stay, unreferenced, for an explicit `pimdir gc` or `sync --reset`. A one-shot tool cannot prompt, so the transition is made non-destructive rather than confirmed.

- **`Side` is gone entirely**, and nothing replaced it. The plan reserved a private `Leg` for the relay path, on the reasoning that streaming a body from its holder to the other one is inherently two-legged. It turned out to be dead code: keying on source names carried every pairwise path, including the relay, so the enum was deleted rather than kept. Report hunks carry source names in text and `--json`, and `sync --source <name>` narrows a run, selecting whole namespaces rather than sources because running half a mirror pushes one way and calls it done.

- **`collection.filter` moved onto the source**, and the account-level table is refused. An account may hold several kinds and a mailbox include-list means nothing to a contacts source. A pair sharing a namespace still applies one filter to both: they bind one set of collections, and filtering them apart would read as a delete on the next pass.

- **At most one source may declare `smtp`**, refused at load rather than resolved by configuration order. The old rule was "left wins", which is a silent tiebreak.

## Deliberate departures from the proposal

- **The wizard did not grow.** The delta had it offering every discovered service and writing one source per accepted one. It keeps its old scope on the owner's call: bare invocation, no configuration found, one account, one backend, offline usage, which is the common case and the only one worth automating. Only the spelling changed, from `left.imap.*` to `imap.*`. Everything else is hand-written.

- **Three or more sources in one namespace are refused**, where the delta said they would keep what crossed. The hub reconciles any number, but the paths that move a body between two are written for a pair. The refusal names the namespace, its sources and the two ways out. Quietly syncing three sources pairwise would have been worse than saying no.

- **No compatibility, config or disk.** v1 is unreleased and nothing runs on the current shape, so `left`, `right`, `store.retention` and `store.hydration` are all refused with their replacement rather than aliased or ignored. Accepting and ignoring `retention = "retain"` on a pairing that derives "keep nothing" would hand back the opposite of what was written.

  A store written before collection ids carried their namespace is not read either, and that needed a guard: every collection would be looked up under a key nothing was written to, and the run would report a healthy sync over an empty replica. `src/offline/state.rs` keeps a `neverest.json` beside the store recording the layout, and a store directory holding a database but no sidecar is refused naming `sync --reset`. The same file carries the previous run's derived value, since nothing else can tell a transition from a steady state.

## Two things the compiler found

`CollectionSourceConfig` merged `namespace` and `filter` into the table that held `create` and `delete`, which were mandatory-when-declared. Anyone writing only a namespace would have been asked for a permission pair they never thought about. Both now default to granting; the `item` table keeps its stricter rule, where declaring half a permission pair is genuinely dangerous.

Reports were carrying the hub id (`mail/INBOX`) where the user expects the name their server uses and the name they typed into `--include-collection`. The display name is stripped at the report boundary, separately from the wire seam.

## Verification

89 unit tests green, `cargo clippy --all-targets --all-features` clean, `cargo fmt`. New tests cover the sugar expanding to the same source id, several sources of one protocol under one account, mail and contacts under one account, the namespace default, the derivation table, a namespace claimed by two kinds, the hub-id and display-name round trip, `--source` selecting whole namespaces, every removed key refused by name, at most one `smtp`, and the sidecar refusing an older store.

The three live-server suites (`carddav`, `duplicates`, `relay`) were ported to the new configuration shape (`sources.left` / `sources.right` sharing a `mail` namespace, and the sugar for the single-source ones) and compile, but are `#[ignore]`d as before and **were not re-run against a server**. The relay path and the namespaced collection ids are therefore unproven end to end.

Capabilities moved: `sync`.
