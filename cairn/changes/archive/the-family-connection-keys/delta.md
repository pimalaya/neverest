---
cairn: delta
change: the-family-connection-keys
---

# Delta

## ADDED Requirements

### Requirement: A DAV source may name its home set
A `carddav` or `caldav` table SHALL take `home`, the address book or calendar home set as a full URL, as cardamum and calendula do: the session SHALL connect to it and list its collections there, discovering no principal and no home set, `server` then being ignored. Without `home`, `server` SHALL be required and the home set discovered from it as before. A table naming neither SHALL fail when the account resolves, naming both keys.

The `auth` table SHALL also take `"none"`, for a server asking for no credential: no `Authorization` header is sent.

#### Scenario: Google's address books
- GIVEN `carddav.home = "https://www.googleapis.com/carddav/v1/principals/<email>/lists/"` and a bearer token
- WHEN the source opens
- THEN its address books are listed under that URL, no `PROPFIND` for a principal sent first

### Requirement: An IMAP source takes the SASL-IR and ID quirks
An `imap` table SHALL take `sasl-ir`, forcing the RFC 4959 initial response on or off, unset following the advertised `SASL-IR` capability; and `id.auto` with `id.fields`, as himalaya and carillon do: with `id.auto = true` the session sends an RFC 2971 `ID` straight after authenticating, each of `id.fields` set to `true` carrying neverest's value for a well-known key (`name`, `version`, `vendor`, `support-url`) and `NIL` otherwise, none at all sending `ID NIL`. Every connection of the source, a reopened one included, opens the same way.

#### Scenario: A server requiring an ID
- GIVEN `imap.id.auto = true` and `imap.id.fields.name = true`
- WHEN the source opens
- THEN the session sends `ID ("name" "neverest")` after authenticating

### Requirement: A connection goes through the configured proxy
An account SHALL take `proxy.url`, `proxy.username` and `proxy.password` (a secret), and so SHALL every backend table and every `smtp` table, as the sibling CLIs do: `url` a `socks5://`, `socks5h://` or `http://` proxy URL, `password` requiring `username`. Every connection a source opens, its send channel's included, SHALL go through the proxy its own table names, else the account's, else the one the `all_proxy` and `https_proxy` environment variables name when the connection opens, else none.

#### Scenario: One source through a SOCKS proxy
- GIVEN an account naming `proxy.url = "http://proxy.example.org:3128"` and one source naming `proxy.url = "socks5h://127.0.0.1:1080"`
- WHEN a sync opens the account's sources
- THEN that source connects through the SOCKS proxy and every other through the HTTP one

## MODIFIED Requirements

### Requirement: A path key expands at deserialize, never at a call site
Every path-valued configuration key (`store.root`, every `tls.cert`) SHALL be shell-expanded by pimalaya-config's own deserializer, a leading `~` and every `$VAR` or `${VAR}` alike, as in every Pimalaya CLI, so a value reaching any call site is already resolved. A key SHALL NOT be expanded at the point it is read: one reader forgetting is a lookup for a literal `./~/…` path, which fails naming a file the user never wrote. A variable that is not set SHALL leave the value as written.

An absent optional key SHALL stay absent rather than expanding an empty path, which is what `#[serde(default)]` beside the deserializer buys.

#### Scenario: A certificate under the home directory is found
- GIVEN `imap.tls.cert = "~/ca.pem"`
- WHEN the configuration is loaded
- THEN the certificate path is the one under the user's home directory

#### Scenario: A store under a variable
- GIVEN `store.root = "$HOME/store"`
- WHEN the configuration is loaded
- THEN the store is the one under the user's home directory

### Requirement: A run reclaims retained items on a schedule
The store retains an item rather than deleting it when its last binding vanishes, so reclaiming is the client's schedule. An account SHALL configure it as `store.purge-after`, a duration as pimalaya-config reads one: an integer of seconds, or a number plus `s`/`m`/`h`/`d`/`w` (`"90d"`), a day being 86400 seconds and months and years refused. **Unset SHALL mean never purge**; `0` or `"0"` SHALL purge immediately, reproducing a terminal delete. There SHALL be no boolean beside it: the delay is the switch.

A sync run SHALL sweep **after** the sync and before the report is finalised, on both the pair and the solo paths, never in a dry run, purging every retained item of the account's own collections whose `retained_at` precedes `now - purge-after` (RFC 3339, the shape the store stamps), one item at a time (`purge`), never another account's: the store's own sweep (`purge_retained_before`) is store-wide, and one account's delay would decide for every other. Sweeping after the sync means an item this run retired starts its delay now rather than being reclaimed by the run that retired it. The sweep SHALL warn rather than fail the run, as the send channel does, and `sync --no-purge` SHALL skip it. The report SHALL carry what was reclaimed (items and bytes) in both the text and `--json` output.

A read-only source (`<protocol>.item.delete = false`, `collection.delete = false`) with no purge delay is therefore a backup: a remote expunge retires the local row without losing the item or its body.

#### Scenario: One account's delay
- **GIVEN** a store shared by an account with `store.purge-after = "0"` and one with none, each holding a retained item
- **WHEN** the first account's retention sweep runs
- **THEN** its retained item is purged, and the other account's stays

### Requirement: The store's owner builds its search index
A build with the `search` feature (on by default) SHALL refresh the store's search index (index.db, pimdir SEARCH §3, §4) after each account a sync runs over, last, for at most the account's `store.index-timeout` (60 seconds unless set, a duration as `store.purge-after` reads one, `0` turning it off), and after a drain, a run performing the queued intents alone and a run declaring collections, for at most 2 seconds. A dry run SHALL NOT refresh. A refresh cut by its deadline SHALL leave the index behind the store and the next refresh SHALL go on from there.

The run SHALL report the refresh under `index`: the changes folded, the bodies newly indexed, the stamps still `behind`, whether another indexer held it (`busy`), and the error of a failed one. An index failure SHALL be a warning and SHALL NOT fail the run or change its exit code: the index is derived from the store.

`neverest index` SHALL build the index of each store the accounts it runs over use, once a store and whole, every account of the store included, until it holds every change the store has, reading no credential and opening no endpoint, and SHALL exit 4 answering `busy` while another indexer holds one.

A reset SHALL leave the index to its next refresh, which folds the removal of every item the dropped collections held, the store counting each (pimdir STORAGE §4.5).

An index of the canonical shape at an earlier search draft SHALL be rebuilt by the next refresh; an index of another shape, or at a later or unknown version, SHALL be refused as `index-version` and left as it is, never rebuilt, written nor deleted (pimdir SEARCH §2), the refusal reported under `index` like any index failure. Every refresh SHALL state the user's zone as the index's floating zone (pimdir SEARCH §7, §8.1): the IANA name of the zone `TZ` names, else of the system's; a zone with no IANA name (a POSIX `TZ` rule) or none found SHALL read as `UTC`, with one warning a run. The first refresh in another zone than the index records re-expands every calendar item.

#### Scenario: A message a sync landed is found by a frontend
- **GIVEN** a sync landing a message whose subject says "Quarterly report"
- **WHEN** a frontend opens the index for queries alone and searches `quarterly`
- **THEN** it finds the message

#### Scenario: A first build longer than the deadline
- **GIVEN** a store whose index a refresh's deadline cut
- **WHEN** the next run refreshes it
- **THEN** it goes on from where the cut left it until the index is current

#### Scenario: A floating meeting in Paris
- **GIVEN** `TZ=Europe/Paris` and a calendar holding a meeting at a floating 09:00 on 7 January
- **WHEN** a run refreshes the index
- **THEN** the index records `Europe/Paris` as its floating zone and places the meeting at 08:00 UTC, 09:00 in Paris


## REMOVED Requirements

None.
