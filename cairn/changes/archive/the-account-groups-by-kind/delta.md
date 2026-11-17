---
cairn: delta
change: the-account-groups-by-kind
---

# Delta

## MODIFIED Requirements

### Requirement: Deciding is a command, never a run
Neverest SHALL NOT decide a content collision during a sync, and SHALL NOT open an editor or any interactive program from one, whatever is attached to its terminal. Deciding SHALL be `neverest conflict resolve` and nothing else.

`--prefer-local` and `--prefer-remote` discard a side, which is what a person may ask for by name and what a background run may never do on its own. `--interactive` SHALL hand the bodies to the merger the conflict's kind names, `contact.merger` for a card and `calendar.merger` for an event, as filesystem paths, base first, then the divergent sides, then the path to write, and SHALL take the result only on a zero exit with that path modified. A non-zero exit, or an untouched output, SHALL leave the conflict exactly as it was: an editor exits zero on a bare quit, and reading that as a choice would discard a side by accident.

No merger SHALL be handed another kind's bodies: a tool reading cards cannot read events, so one command for every kind fails on half of them. A kind naming no merger SHALL be refused naming its key, and a mail conflict SHALL be refused outright, mail bodies being immutable.

A tty is not consent. A run has one when it is driven by a wrapper script, when it is watched from a pane nobody is sitting at, and when it is a person waiting, and the three are indistinguishable from inside. Escalating on that signal blocks every remaining collection behind a human who may not exist.

#### Scenario: A sync with a terminal still parks
- GIVEN a run attached to a tty that marks a collision
- WHEN the run continues
- THEN no program is spawned, the conflict parks, and the remaining collections reconcile

#### Scenario: An aborted merger changes nothing
- GIVEN an interactive resolution whose merger exits non-zero, or leaves its output untouched
- WHEN it returns
- THEN the conflict is unchanged and nothing is pushed

#### Scenario: A card never reaches the calendar's merger
- GIVEN an account naming `calendar.merger` and no `contact.merger`, and a parked card
- WHEN the card is resolved interactively
- THEN it is refused naming `contact.merger`, nothing runs, and the card stays parked


### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `gpeople`, `gcal`) SHALL be sugar for `sources.<key>.<key>`, the source taking the key as its name. The `msgraph` table SHALL be sugar for one source a domain it names, each named after the table and the domain: `msgraph-mail`, `msgraph-contact` and `msgraph-calendar`, each holding the table's session and that domain's table alone. The sugar SHALL produce a configuration indistinguishable from the expanded form, source id included, so expanding it by hand is a no-op on the store.

A key names a protocol, and where a protocol carries several domains a table under it names the domain: `carddav` and `caldav` are two keys over one adapter, `msgraph.mail`, `msgraph.contact` and `msgraph.calendar` three tables over one session. The domain is what the kind falls out of, so it SHALL be distinct per kind even when the transport is shared.

Declaring the same key both directly and under `sources` SHALL be a configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

#### Scenario: Expanding the Graph table changes nothing
- GIVEN an account written as `msgraph.auth.token.raw = "..."` with `msgraph.mail = true` and `msgraph.contact = true`
- WHEN it is rewritten as `sources.msgraph-mail.msgraph.mail = true` and `sources.msgraph-contact.msgraph.contact = true`, each with the token
- THEN the sync opens the same two source ids and reuses every existing binding

## REMOVED Requirements

### Requirement: One Graph table keeps the source ids of the three it replaced
The Graph sources are named after the table and the domain (`msgraph-mail`, `msgraph-contact`, `msgraph-calendar`), and v1 reads no 0.x store, so no id needs keeping.
