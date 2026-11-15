---
cairn: delta
change: sync-progress-is-events
---

## ADDED Requirements

### Requirement: A sync prints its progress as events
Under `--json`, `neverest sync` SHALL print its progress as events before its report, one line each, each an object carrying `event`: `opened` (`endpoint`) once an endpoint's first connection is open, `listed` (`endpoint`, `collections`) once it listed its collections; for a pair, `collection-started` (`source`, `target`, `collection`, `position`, `total`), `collection-progressed` (`source`, `target`, `collection`, `done`, `total`) and `collection-finished` (`source`, `target`, `collection`, `error`, `null` unless the collection stopped short); for a source with no target, `scan-progressed` and `download-progressed` (`source`, `done`, `total`), each printed as its phase starts. A progress count SHALL be printed at most once per percent of its total, a new count of a collection's next phase starting again. Events SHALL report progress only: everything the run did or left SHALL stay in the report, the last line. The events SHALL be registered as `neverest-sync.events`.

Text mode SHALL print no event, the spinner on stderr being their text form; under `--json` no spinner SHALL be drawn.

#### Scenario: A frontend follows a long download
- GIVEN an account with no target whose store owes 2 bodies
- WHEN `neverest sync --json` downloads them
- THEN it prints `download-progressed` with `done` 0, 1 and 2 of `total` 2, then the report

#### Scenario: A script reads the report alone
- GIVEN a sync piped to a script
- WHEN the run succeeds
- THEN the last line is the report, without an `event` key, and every line before it carries one

## MODIFIED Requirements

### Requirement: The spinner reports in-mailbox progress
In text mode, while syncing a mailbox, the spinner SHALL report progress through the slow inner phase, body hydration (and, under relay, the relayed messages), as a percentage appended to the mailbox line (`[2/7] Syncing INBOX 66%`), updated per streamed `Full` body. Fast phases (enumerate, the `Meta` upgrade) stay silent. The progress tick is invoked from the concurrent fetch pool and is safe to call from several threads at once. Under `--json` the same ticks print `collection-progressed` events instead.

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema` for the registry, and SHALL print it once. It SHALL NOT report data as a message: a message serialises as one prose string, so `--json` over one yields nothing a consumer can read, and several messages in one run yield several documents where a consumer expects one value.

`configure` returns data: the account it generated, under the name and default claim it derived. Its `Display` is the TOML document alone, which is what makes a redirected stdout a usable configuration file.

Every such output type SHALL be registered in the schema registry under its invocation path, the command path joined with hyphens and prefixed `neverest-`, as printed (the serialize contract, so a key always printed is required, `null` or not), and `neverest json-schema` SHALL print one schema to the standard output or write one file per command into a directory. A test SHALL run pimalaya-cli's registry check over the command tree: every runnable command described, bare `neverest` aside (it shows the help or runs the wizard `configure` describes), and every key naming a command, `neverest-error` aside, so a renamed subcommand cannot leave a schema nobody can ask for.

#### Scenario: A notifier reads the sync payload from its schema
- GIVEN a build of neverest
- WHEN `neverest json-schema neverest-sync` runs
- THEN it prints the schema of the sync report, naming `conflicts` and `outstandingConflicts` among its fields

#### Scenario: A checked account is one JSON document
- GIVEN a configured account
- WHEN `neverest check --json` runs
- THEN one document is printed, naming the account, its mode and every endpoint that answered
