---
cairn: delta
change: machine-output-contract
---

## MODIFIED Requirements

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema` for the registry, and SHALL print it once. It SHALL NOT report data as a message: a message serialises as one prose string, so `--json` over one yields nothing a consumer can read, and several messages in one run yield several documents where a consumer expects one value.

`configure` returns data: the account it generated, under the name and default claim it derived. Its `Display` is the TOML document alone, which is what makes a redirected stdout a usable configuration file.

Every such output type SHALL be registered in the schema registry under its invocation path, the command path joined with hyphens and prefixed `neverest-`, and `neverest json-schema` SHALL print one schema to the standard output or write one file per command into a directory. A key naming no command, `neverest-error` aside, SHALL be refused, so a renamed subcommand cannot leave a schema nobody can ask for.

#### Scenario: A notifier reads the sync payload from its schema
- GIVEN a build of neverest
- WHEN `neverest json-schema neverest-sync` runs
- THEN it prints the schema of the sync report, naming `conflicts` and `outstandingConflicts` among its fields

#### Scenario: A checked account is one JSON document
- GIVEN a configured account
- WHEN `neverest check --json` runs
- THEN one document is printed, naming the account, its mode and every endpoint that answered

## ADDED Requirements

### Requirement: The JSON contract is described and versioned
`neverest json-schema` SHALL describe, under the key `neverest-error`, the failure object every command prints under `--json`. Every generated schema SHALL carry an `$id` naming the contract version, which is the minor version while Neverest is below 1.0 (`v0.3`) and the major version after. Every registered payload SHALL be a JSON object, checked by a test. Piped `--json` output SHALL print one value per line.

#### Scenario: A caller reads a failure against its schema
- GIVEN `neverest json-schema neverest-error`
- WHEN it runs
- THEN the schema of the JSON failure object is printed, its `$id` being `https://pimalaya.org/schemas/neverest/v0.3/neverest-error.json`
