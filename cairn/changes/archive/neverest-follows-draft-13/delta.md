---
cairn: delta
change: neverest-follows-draft-13
---

# Delta

## ADDED Requirements

### Requirement: A change refused as it stands is held and reported
A connector SHALL answer `Refused { reason }` (pimdir SYNC §4) for a change the remote refuses as it stands, whatever its base: an HTTP status from 400 to 499 naming the request (on DAV, Graph mail, contacts and calendars, Google Calendar, People and Gmail), all but 401 and 407 (the credentials), 408, 423, 425 and 429 (a state that passes) and 409 and 412 (a stale base); an IMAP `NO` naming `CANNOT`, `PARSE`, `TOOBIG` or `NOPERM`; and a rule the change breaks that neverest states before any request, coded `unsupported` (Graph's append outside Drafts, an override Google Calendar or Graph takes through no instance). `reason` SHALL be the failure's messages joined by `: `. A failure naming a quota exceeded or a rate limit, whatever its status, SHALL NOT be a refusal: the account's state, not the change's, which the same change outlives once the state passes, where a refusal holds it for good. Every other failure (a bare IMAP `NO`, a `BAD`, a `5xx`, a transport failure) cannot tell, and SHALL stay `Rejected` as before.

The engine records a refusal on the item's binding and holds the change while it is derived under the same key, pushing nothing, and reports it every run (pimdir SYNC §5). The report SHALL list each under `refusals` (`side`, `collection`, `id`, `reason`), the `id` of a create being its key with no control character, once a run however many passes meet it, counted as a warning, the run exiting 3, and SHALL take back the hunk the run derived for that item, as for a rejected write. A refusal SHALL NOT also be reported under `rejected`. The text output SHALL print each among the warnings. Under `--json`, `sync` SHALL print a `refused` event for each, once the account's run ends. The change leaves the hold when the item changes (an edit, a flag, a move into a collection the source takes), which pushes it anew, or when it is removed, which withdraws a create whatever the source's rights (pimdir STORAGE §15.6).

#### Scenario: A message appended to a Graph folder other than Drafts
- **GIVEN** a message staged into a Graph `Archive` folder by another source of the account
- **WHEN** the account syncs twice
- **THEN** the first run answers the append `Refused` with "appends to Drafts only", sending nothing, the second pushes nothing, and both report it under `refusals` and exit 3

#### Scenario: An IMAP append refused as it stands, and one refused for a quota
- **GIVEN** a pending create the server answers `NO [CANNOT]`, and another it answers `NO [OVERQUOTA]`
- **WHEN** the account syncs twice
- **THEN** the first is appended once, held and reported under `refusals` by both runs, and the second is appended by both runs and reported under `rejected`

## MODIFIED Requirements

### Requirement: The header of the sync capability
The store and the engine follow pimdir storage-draft-13, sync-draft-13 and search-draft-13 (io-pimdir 0.14; pimdir 1c6442e), where they followed draft-11 (io-pimdir 0.12).

### Requirement: A store the format outgrew is refused with its remedy
The remedy reads "then rebuild it (`neverest init`, then a sync)"; the scenario names `storage-draft-11`, as io-pimdir 0.13 wrote it, and its message the rebuild with `neverest init`.

### Requirement: A refused write is reported, never counted as applied
It covers a write the remote did not refuse as it stands, answered `Rejected` and reported under `rejected`; the scenario's refusal is a 412.

### Requirement: A sync prints its progress as events
Adds `refused` (`source`, `collection`, `id`, `reason`), one for each row of the account's `refusals`, printed once the account's run ends; events report progress and the refusals.

### Requirement: Graph mail moves, copies and adds drafts
An append outside Drafts is refused as it stands, coded `unsupported`, and held; an item queued into such a folder and never pushed is removed whatever the source's `item.delete` (pimdir STORAGE §15.6), the withdrawal taking its refusal. The "A draft staged in Drafts" scenario follows.

### Requirement: A mail sync lists within a scope on the `Date`
A round over the band a coverage lacks states `narrowed` and hands the enumeration on its last page too (pimdir SYNC §5, vectors 160 and 161), where it handed none; the engine finds absent only a member in scope the enumeration lacks.

### Requirement: A provider narrows a scope with a margin
The engine finds a member absent through the enumeration alone; a Graph band round's enumeration is a fresh delta of the folder with ids, dates and flags, its link dropped.

### Requirement: A Graph mail folder keeps one delta link over the whole folder
A band round keeps the link and hands the folder's ids by a fresh delta, whose link it drops; the "Three widenings" scenario passes over the folder once for the link and once a band for its ids.

### Requirement: A capped source says so, and keeps what it leaves unlisted
`unlisted` also carries the engine's count for a narrowed round handing no enumeration, which no narrowed round of neverest's connectors does.

## REMOVED Requirements
