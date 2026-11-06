---
cairn: delta
change: every-key-is-printed
---

## ADDED Requirements

### Requirement: Every key is printed
Every output type neverest prints SHALL print every key it declares: a list as `[]` when empty, an absent value as `null`, a bool as `false`, never leaving a key out. Its schema SHALL mark each such key required. State neverest writes for itself and reads back (the sidecar beside the store) is not output and MAY leave keys out.

#### Scenario: A quiet run still names its lists
- GIVEN a sync that parked, refused and conflicted nothing
- WHEN `neverest sync --json` runs
- THEN the report holds `conflicts`, `parked`, `refused` and `unreached`, each `[]`, and `purged` as `null` when no sweep ran

### Requirement: The mode is data
`check` and `init` SHALL print the account's mode as an object, `{sources, targets, oneWay, retain}`: the source and target names sorted, whether the sources are authoritative, and whether the store keeps bodies. The sentence describing it SHALL be its text form only.

#### Scenario: A caller reads the direction of an account
- GIVEN an account with a CalDAV source and a target `backup`
- WHEN `neverest check --json` runs
- THEN `mode` is `{"sources": ["caldav"], "targets": ["backup"], "oneWay": false, "retain": false}`

## MODIFIED Requirements

### Requirement: A check lists collections with their roles
`neverest check` SHALL list, for each endpoint, every collection it answered with: its id, its name and its role among `inbox`, `sent`, `drafts`, `trash`, `junk`, `all`, `archive`, `flagged` and `important`, `null` when the server states none. A role SHALL come from the server alone, never from a collection's name: on IMAP the SPECIAL-USE attributes of the `LIST` rows (RFC 6154) and `INBOX` (RFC 3501 §5.1); on Graph mail the well-known folders, one lookup per role, a well-known folder the mailbox does not have being skipped; on the Gmail API the system labels. Other backends state no role. The check SHALL write nothing to the store. It is a doctor: what it prints is for a person, and a program reads roles from the store a sync fills.

Display names are localised (`[Gmail]/Bin`, "Éléments envoyés") and an IMAP server need not name its folders after their use: only the server's statement lets a caller propose the right folder for each use. On Graph, the Drafts folder is the one place a message can be created, so a caller has to know which listed folder it is.

#### Scenario: An IMAP check names the special-use folders
- GIVEN an IMAP account whose server advertises SPECIAL-USE
- WHEN `neverest check --json` runs
- THEN each source lists its collections, `INBOX` with the role `inbox` and the sent, drafts and trash folders with theirs

#### Scenario: A folder without a statement has no role
- GIVEN a folder the server marks with no SPECIAL-USE attribute
- WHEN `neverest check --json` runs
- THEN the folder is listed with `role: null`, whatever its name

### Requirement: A check names the default calendar and address book
`neverest check` SHALL mark with `default: true` the collection of an endpoint where a new item goes when none is named, as the server states it: on Google Calendar the calendar the calendar list marks `primary`; on Google People the one address book; on a Graph calendar source the calendar with `isDefaultCalendar`; on a Graph contacts source the default Contacts folder; on CalDAV the calendar the principal's scheduling inbox names as `schedule-default-calendar-URL` (RFC 6638 §9.2), when the server gives it. CardDAV states none, nor does mail, whose folders have roles. At most one collection per endpoint SHALL carry the mark, every other `default: false`. A failed CalDAV scheduling lookup SHALL be no default, never a failed check.

A caller choosing one calendar and one address book for an account takes the marked one, and the first listed when none is marked.

#### Scenario: A CalDAV server with scheduling
- GIVEN a CalDAV account whose scheduling inbox names its default calendar
- WHEN `neverest check --json` runs
- THEN that calendar is listed with `default: true`, and no other

### Requirement: A check reports what each source declares it can do
`neverest check` SHALL list, for each source, the capabilities a sync declares for it in the store (pimdir STORAGE §15.6) as `capabilities: [{ name, support, detail }]`, one row per capability, `support` being `full`, `partial` or `none` and `detail` why it is partial or refused, `null` otherwise, from its backend and configured rights alone. An intent a source performs on the collections it holds (`calendar.reply`, `calendar.cancel`, their `.occurrence` options, `collection.create`, `collection.rename`, `collection.delete`), which the store gets as a `none` source-wide row and one row per collection, SHALL read as it does on those collections: the store's collections are unknown to a check, so the `none` a rename or a delete gets on a collection stating a role is not listed. The source-wide `calendar.component.vtodo` row a sync declares from the configuration (a Google Calendar or Graph calendar source, or a source one of whose targets is one) SHALL be listed with them. What a CalDAV listing states of a calendar SHALL be listed on that collection as `takesTasks`, `false` for a component set without `VTODO`, `true` for one naming it, `null` where no set is stated, on a target's collections as on a source's: a sync declares it as `calendar.component.vtodo` `none` on the source's calendar of that name. A target and an undeclared (JMAP) source SHALL list no capability. A capability a source learns to declare is reported by the check with no change to it.

#### Scenario: A check names the calendars that take no tasks
- **GIVEN** a CalDAV source whose server lists `events` with `VEVENT` alone and a Google Calendar target
- **WHEN** `neverest check --json` runs
- **THEN** `events` is listed with `takesTasks: false`, and the source's capabilities hold `calendar.component.vtodo` `none` naming the target
