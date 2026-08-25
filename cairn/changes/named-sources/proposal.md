---
cairn: change
id: named-sources
status: landed
created: 2026-08-26
---

# An account holds named sources, not a left and a right

## Why

Setting up a Fastmail account, the wizard asks for a protocol and you pick IMAP. Then you want your contacts too. The engine can do it: pimdir already keys a collection by its media type, and io-replica already reconciles N sources against one hub. The configuration cannot express it, because an account is a `left` and a `right` and both are forced to agree on their kind.

`left` / `right` conflates three things that are not the same thing:

1. **Source identity**, which is positional and arity-2. The pimdir source id is literally the string `"left"` or `"right"` (`src/offline/mod.rs`, `source_id`).
2. **Kind**, derived from the backend and required to match across the two sides. This is what blocks mail and contacts under one account.
3. **Topology**, which is implicit in there being exactly two sides. One side means local sync, two means remote-to-remote.

Defect 2 is the one that bites today. Defect 1 bites next: a backup fan-in (two IMAP servers into one store), a migrate (imap A to imap B alongside contacts), and a merged contacts view (Fastmail plus Google) all need more than two sources, or two sources of the same protocol, or both.

Defect 3 is not really a defect, it is a heuristic that has run out. "Side count selects the sync mode" works at arity 2 and says nothing at arity 5 with three kinds in the mix.

## What this is not

It is not a mode discriminant. There is no `mode = "mirror" | "cache"` here. The engine has exactly one operation, reconcile one source against the hub, and a mode field would be a configuration concept with no engine behind it. It also goes wrong the moment one account holds one mail source and two contacts sources, which the shape below allows.

It is also not two accounts per provider. That is the do-nothing baseline (`fastmail-mail`, `fastmail-contacts`, `fastmail-calendar`), and it costs duplicated credentials and discovery, three separate stores, and a frontend that must union three accounts to show one provider. The android plan wants the opposite: one store for three domains.

## Shape

An account stays the hub, stays the database, stays what `-a` selects. Sources live inside it, keyed by name:

```toml
[accounts.personal]
sources.fastmail.imap.server = "imap.fastmail.com"
sources.fastmail.imap.sasl.plain.username = "..."
sources.gmail.imap.server = "imap.gmail.com"
sources.fastmail-dav.carddav.server = "https://carddav.fastmail.com"
```

The single-provider case, which is most of them, gets sugar: a backend written directly under the account is a source named after its protocol.

```toml
[accounts.fastmail]
imap.server = "imap.fastmail.com"
carddav.server = "https://carddav.fastmail.com"
caldav.server = "https://caldav.fastmail.com"
```

is exactly

```toml
[accounts.fastmail]
sources.imap.imap.server = "imap.fastmail.com"
sources.carddav.carddav.server = "https://carddav.fastmail.com"
sources.caldav.caldav.server = "https://caldav.fastmail.com"
```

The sugar's source id is the protocol name, which is the same id the explicit form writes. That is load-bearing, not cosmetic: a source id names every binding that source owns in the store, so renaming one orphans them all. Expanding the sugar by hand must be a no-op on disk, and it is.

## Merged and isolated

Two sources of the same kind in one account are ambiguous on their face. Do they mirror each other, or do they sit side by side in the store as two independent caches? The answer is not a mode, it is whether they bind to the same hub collection.

Isolated, which is the default, with two mail sources both holding an `INBOX`:

```
mail / fastmail / INBOX     item A, item B          (bindings: fastmail)
mail / gmail    / INBOX     item C                  (bindings: gmail)
```

Syncing `fastmail` touches the first collection only. Deleting A on Fastmail drops it there and Gmail never hears about it. Nothing is ever pushed between the two, because no collection has a binding gap to fill.

Merged, both sources declaring `collection.namespace = "mail"`:

```
mail / mail / INBOX         item A   (bindings: fastmail, gmail)
                            item B   (bindings: fastmail)
                            item C   (bindings: gmail)
```

Syncing `gmail` now sees item B in a collection Gmail participates in, with no Gmail binding. That gap is the propagation mechanism, all of it: the engine pushes B to Gmail and records the binding, and the same in reverse for C. Delete A on Fastmail, its Fastmail binding goes, the engine sees the object still bound elsewhere and pushes the delete to Gmail too.

So the hub collection key becomes `(kind, namespace, name)`, and the namespace defaults to the source name. Sharing a value is the explicit act of saying "these two are the same thing".

Local creates attribute themselves through the same mechanism, with no owner field anywhere. In isolated mode a new card is created in `vcard/fastmail-dav/Default`, so it lands on that source and nowhere else. In merged mode it is created in the shared collection and goes to every source in that namespace whose permissions allow a create.

### Why isolated is the default

Merged would preserve today's `left` / `right` behaviour for free, which is tempting. Its silent failure mode is copying Gmail's INBOX into Fastmail on first sync, against two real providers, and it is not cheap to undo. Isolated's silent failure mode is a mirror that did nothing, which shows up in the report and costs a config line to fix. Take the harmless one.

There is no compatibility path softening the flip, and that is deliberate: an alias injecting a shared namespace behind the user's back is the same implicit behaviour this change exists to make explicit.

## Retention is derived and reported, not configured

The two situations that started this discussion, pass-through mirror and local offline cache, are not settings a user picks. They are what falls out of how many sources share a namespace and whether the pairing can stream. So `store.retention` and `store.hydration` leave the configuration surface, and what the store keeps is derived per kind:

- one source in the namespace: keep **every** body, because nothing crosses and anything less makes the store an index rather than a replica;
- exactly two sources sharing a namespace on a pairing that can stream (mail, IMAP to IMAP): keep **no** body, streaming each crossing through the bounded pipe, which is today's relay default for a migrate;
- anything else, every DAV pairing included: keep **the bodies that crossed**. A namespace of three or more sources is refused outright: the hub reconciles any number, but the paths that move a body between them are written for a pair.

This gives up two configurations that were expressible: a mirror that also keeps a local offline copy, and a single source kept as an envelope-only index. Both are niche, and an override can be added later without breaking anything, where removing one could not.

It also deletes an error class. An explicit `relay` on a DAV pairing used to fall back to retain silently, handing the user the opposite of the disk guarantee they asked for. With nothing explicit to honour, there is nothing to silently substitute.

The cost of deriving is that the derived value moves when the configuration moves, and one of those transitions is destructive: adding a second source flips a kind from keeping every body to keeping none, dereferencing everything already stored. The derivation therefore governs only what the sync fetches and keeps **from now on**, and never retroactively drops what is on disk. Existing objects stay, become unreferenced, and are reclaimed only by an explicit `pimdir gc` or `sync --reset`. Non-destructive by construction beats a confirmation prompt in a one-shot tool.

What replaces the setting is the report. Every run states, per kind and namespace, how many sources it holds and what the store keeps for them, and `check` states the same before a first sync ever runs, deriving it without needing a server to answer. A transition is called out by name, with what became unreferenced and how to reclaim it. Someone who set up a two-source backup expecting the store to *be* the backup learns it on run one, not on the day they need it.

## Consequences beyond the config keys

- **Collection ids gain the kind and the namespace.** Today it is the collection name as the bare collection id. A CardDAV book named `Default` and a mailbox named `Default` would collide in one store.
- **`Side` goes away.** `Side::other()` is only meaningful at arity 2. Report hunks and the per-source index key on the source name.
- **The SMTP "left wins" tiebreak goes away with it.** At most one source per account may declare `smtp`; more is a configuration error. A silent tiebreak was already a smell.
- **`collection.filter` moves onto the source.** It is account-level and symmetric today, and an `include = ["INBOX", "Sent"]` is nonsense for a contacts source. This gives up the symmetry guarantee, which is a real trade and not free: asymmetric filters mean a collection synced on one source and skipped on another, which is useful for a migrate and surprising otherwise.
- **`store.retention` and `store.hydration` leave the configuration.** What the store keeps is derived per kind and reported, never picked. A configuration still carrying them is refused, naming the derived value that replaces it.
- **`sync` takes one account.** The database is never in question, because the account names it. `--source <name>` narrows which sources run inside the same database.
- **The wizard keeps its scope.** One account, one source, offline usage, on bare invocation with no configuration found. Only its spelling changes, to the direct-backend sugar. Everything past one source is manual config.

## Migration

There is none, and that is a decision rather than an oversight. v1 is unreleased and nothing is deployed on the current shape, so this change lands inside the v1 rewrite rather than after it.

- **No config aliases.** `left` and `right` are refused at load, naming `sources` and the shared `collection.namespace` that reproduces a mirror. An alias would exist only to inject that namespace implicitly, which is the behaviour this change exists to make explicit.
- **No store compatibility.** Collection ids gain the kind and the namespace, and a store written on the previous shape is not read. No mapping, no `--reset` dance for anyone, because there is nobody to migrate.
- **No accepted-but-ignored keys.** `store.retention` and `store.hydration` are refused the same way, each naming the derived value that replaces it. Silently ignoring `retention = "retain"` on a pairing that now derives "keep nothing" would hand the user the opposite of what they wrote.

One rule for all three: a key that no longer exists is refused with the reason and its replacement. That costs nothing here and leaves no compatibility path to carry forever.

The v0 to v1 upgrade path in MIGRATION.md is unaffected in shape. It already tells its reader to port accounts by hand against `config.sample.toml`, so it only needs to describe sources instead of sides.

## Alternatives considered

**Flat account as a single source, store named separately.** `[sources.fastmail-mail] store = "personal"`. Shorter keys, but the hub still owns retention, hydration, purge-after and root, so a `[stores.*]` table has to exist anyway. That leaves two top-level tables plus a foreign key that can dangle or be misspelled. Nesting enforces the grouping structurally and costs one level of TOML.

**Account as an enum over backends.** That is the right shape, one level down, and it is already what `SideBackendConfig` is. Applied to the account it renames "side" to "account" and loses the container, which then has to be reinvented to answer which database to open.

**Flat multi-protocol account with no source table.** The sugar above, and only the sugar. It cannot express two sources of one protocol, which the backup, migrate and merged-contacts cases all need.
