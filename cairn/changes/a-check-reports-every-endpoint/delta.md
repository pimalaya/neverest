---
cairn: delta
change: a-check-reports-every-endpoint
---

## ADDED Requirements

### Requirement: A check reports every endpoint
`neverest check` SHALL check every endpoint the account declares whatever the others answer, and SHALL print one row per endpoint, sorted by name: `source`, `backend` (its protocol table), `ok`, `code` and `error` (`null` when it answered), then `collections` (empty when it did not answer), `capabilities` and `smtp`. A credential that does not resolve SHALL fail its endpoint's row alone. When an endpoint or its send channel did not answer, the command SHALL print the report and exit 2, never the failure object.

A frontend adding an account shows what works and what does not in one run, rather than one failure per attempt.

#### Scenario: Two endpoints that do not answer
- GIVEN an account whose source and target both point at a closed port
- WHEN `neverest check --json` runs
- THEN it prints both rows, each with `ok: false`, its `error` and no collection, and exits 2

## MODIFIED Requirements

### Requirement: A check opens the send channel too
`neverest check` SHALL open the SMTP channel an endpoint declares, upgrade it and authenticate, then quit without sending. Each endpoint's row SHALL carry the channel's own check row as `smtp`, `{backend: "smtp", ok, code, error}`, `null` when the endpoint declares no channel, when its credentials did not resolve, or in a build without SMTP. A channel that does not open SHALL fail its row and make the check exit 2, the other endpoints still checked.

A send channel holds its own server and often its own credential. Checking the sources alone left a wrong SMTP password to surface at the first send, a queued submission parked for a reason the check had said nothing about.

#### Scenario: A wrong submission server fails the check
- GIVEN an account whose IMAP source answers and whose SMTP channel does not
- WHEN `neverest check --json` runs
- THEN the source's row is `ok: true` with its collections, its `smtp` row `ok: false` naming why, and the check exits 2

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
- THEN one document is printed, naming the account, its mode and every endpoint, answered or not
