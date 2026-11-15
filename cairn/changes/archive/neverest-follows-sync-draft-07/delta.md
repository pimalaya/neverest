---
cairn: delta
change: neverest-follows-sync-draft-07
---

# Delta

## ADDED Requirements

### Requirement: A fetched divergence settles by the endpoint's policy

Every upgrade neverest runs (a page's or phase 2's body landing, a full hydration, the copy hydration of a pair, the fetch of a diverging body before a merge or an overwrite) SHALL take the options of the endpoint's pushing sync (`PimdirUpgradeOptions::from(&sync_options)`): its conflict policy and whether it may push content, a dry run or an endpoint that writes nothing back pushing none. An upgrade fetching the body of a divergence then settles it by that policy (pimdir SYNC §6): a one-way target's `prefer-local` keeps the store's body, owed to the target, and the target's own as a retained item; a one-way source's `prefer-remote` adopts the source's; a two-way endpoint's `manual` records it for the merge. A body an upgrade kept and did not adopt SHALL be reported under `overruled` with the endpoint and the item's handle, whichever upgrade kept it.

#### Scenario: A one-way target's edit read late
- **GIVEN** a one-way account whose target edited a card the source changed, the target's pull parking the divergence with no body
- **WHEN** the run fetches the target's body
- **THEN** the fetch settles it: the store's body is owed to the target, the target's own is a retained item, and `overruled` names it

### Requirement: A store is its user's alone

A store another user can read or write is refused by every profile as `store-exposed` (pimdir STORAGE §18). Every store directory neverest makes SHALL be its user's alone, so its own readers and frontends open it: the store `init` creates, which io-pimdir restricts at creation whatever the umask; the replica a dry run copies the store into, created `0700` before the copy, its files keeping the store's modes; and the directory `sync --reset --force` moves a store into.

#### Scenario: A store made under umask 022
- **GIVEN** a shell whose umask is 022
- **WHEN** `neverest init` creates an account's store
- **THEN** its directory and pimdir.db grant nothing to group or others, and a reader and a producer open it

#### Scenario: A dry run under umask 022
- **GIVEN** a store and a shell whose umask is 022
- **WHEN** `sync --dry-run` runs
- **THEN** its replica opens as a store

## MODIFIED Requirements

### Requirement: A mail sync lists within a scope on the `Date`

An account MAY bound its mail by `item.filter.since`, and a run by `sync --since`, which overrides it: a duration back from today (`30d`, `12w`, `6mo`, `1y`), taken to the start of its UTC day so the runs of one day list one scope, a date at midnight UTC, or an RFC 3339 instant. A mail collection SHALL then sync under the scope from that floor (pimdir SYNC §5): a listed message whose `Date` lies below it SHALL be left out of the page, a message with no usable `Date` SHALL be in every scope, and an explicit removal SHALL apply whatever the date. What a scope leaves out SHALL NOT be deleted, neither in the store nor on the server.

A round whose listing a provider's date filter narrowed (IMAP `SENTSINCE` and `SENTBEFORE`, Gmail's `after:` and `before:` on the arrival, Graph's `sentDateTime`) SHALL state it on every page (`narrowed`), and its last page SHALL hand every handle the collection holds, read by an enumeration no date filter narrowed (`enumerated`, pimdir SYNC §4, §5): the engine then finds absent only a member dated before the round opened or one the enumeration lacks, so an undated draft received before the window and a message whose `Date` runs ahead of its arrival stay in the store. A round over the band a coverage lacks SHALL state `narrowed` and hand no enumeration: the engine finds absent there only a member dated in the band, below the round's opening, and never one with no usable `Date`, which an enumeration could not keep further. A round over the whole collection, a delta and a head SHALL state neither. A scope SHALL be refused, by the key or flag it came from, when an endpoint the run syncs holds contacts or calendars.

The carve-out "A round over the band a coverage lacks SHALL infer no delete of a member with no usable `Date` it did not list" is gone from this requirement: it is the engine's (`finds_absent`, io-pimdir src/sync.rs), the band round's `narrowed` changing nothing there. The scenarios stay, with one added:

#### Scenario: What an arrival filter misses
- **GIVEN** a Gmail inbox the store binds, holding an undated draft received before the window and a message dated 2030 received before it, which `after:` lists neither of
- **WHEN** a round under `--since` closes, its last page handing the label's ids
- **THEN** both stay in the store

### Requirement: A provider narrows a scope with a margin

The margin paragraph is unchanged. Added:

The enumeration a narrowed round's last page hands SHALL be read after that page's listing, so a member arrived since is in it: IMAP's `UID SEARCH ALL` on the selected mailbox, one command a round; Gmail's ids of the label, listed with no query, 500 ids a request at 5 quota units each, once a round; Graph's ids of the folder's delta pass, the scoped round's last page, which reads them already and costs no request more.

### Requirement: A store the format outgrew is refused with its remedy

A store opens under its own format version alone (pimdir STORAGE §4.2, §6): one any earlier neverest wrote, naming no `store_meta.format_version` or an earlier draft, is refused as `store-version` and never migrated. Opening one SHALL fail naming the reinstall in the order that keeps its queued writes, which live only in that store: sync it with the neverest that wrote it until nothing is queued, then `neverest sync --reset --force -a <account>`, which moves it aside and syncs a new one, rather than surfacing the raw refusal alone. The message SHALL never say to delete the store. What the old store held that no server holds (retained items, whatever was still queued) stays in the store moved aside. MIGRATION.md SHALL give the same order.

#### Scenario: A store an earlier neverest wrote
- **GIVEN** a store whose `store_meta.format_version` names `storage-draft-07`
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 with `code` `store-version`, the message naming the sync with the previous neverest until nothing is queued and `neverest sync --reset --force -a <account>`, and no delete

### Requirement: A store a newer neverest wrote asks for an upgrade

Opening a store refused as `store-version` whose `store_meta.format_version` names a later draft than this neverest's, or any version not of the `storage-draft-NN` form (a release), SHALL fail saying a newer neverest wrote it and naming the upgrade, never a reset. The scenario stays.

### Requirement: A store error carries its code

The relayed codes are `store-version`, `store-uncreated`, `store-foreign`, `store-corrupt`, `store-not-current`, `store-hash-algo`, `sync-version`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `store-exposed`, `action-malformed`, `flag-invalid`, `unsupported`, `index-fts5-missing`, `index-version`; `store-newer`, `store-older` and `store-stale` are gone with io-pimdir's. A malformed action now carries its code. Left out, beside the earlier ones: `index-zone-unknown` (neverest keeps the floating zone `UTC`) and `conflict-unread` (the one-way overwrite fetches the unread body). The remedies add the reinstall. The downgrade scenario now restamps `store_meta.format_version` to a later draft and expects `store-version`.

### Requirement: A listing missing most of a collection waits for a person

A round, or a rebuild, whose listing names no member while any bound item is absent parks too, however few (pimdir SYNC §5, §8), reported under `absences` and exiting 2 as a mass absence does. Added the scenario "An address book listed empty".

### Requirement: A one-way overwrite keeps the body it discards

The fetch of an unread target body settles it by the target's `prefer-local` itself (see "A fetched divergence settles by the endpoint's policy").

### Requirement: A reset never loses work no server holds

The directory `--reset --force` moves the store into is created its user's alone, as the store was, so the moved store still opens as one.

### Requirement: The store's owner builds its search index

Added: an index of the canonical shape at an earlier search draft SHALL be rebuilt by the next refresh; one of another shape, or at a later or unknown version, SHALL be refused as `index-version` and left as it is (pimdir SEARCH §2), reported under `index` like any index failure; floating times are read in io-pimdir's default zone (`UTC`).

### Requirement: A calendar its server keeps from tasks is declared so

The capability is `calendar.component.vtodo` (pimdir STORAGE §15.6, Annex B), where it was `x-vtodo`; likewise in "A check reports what each source declares it can do" and "Every source declares its capabilities before the drain".

### Spec header

The spec names the drafts it cites: pimdir storage-draft-08, sync-draft-07 and search-draft-08 (io-pimdir 0.8 at bff5737).
