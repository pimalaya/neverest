---
cairn: delta
change: data-commands-describe-what-they-print
---

## ADDED Requirements

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type
deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema`
for the registry, and SHALL print it once. It SHALL NOT report data as a
message: a message serialises as one prose string, so `--json` over one yields
nothing a consumer can read, and several messages in one run yield several
documents where a consumer expects one value.

A command that only confirms SHALL keep its message. `configure` says an
account was configured and reports nothing about it, which is the whole of the
exception.

Every such output type SHALL be registered in the schema registry under its
invocation path, the command path joined with hyphens and prefixed `neverest-`,
and `neverest json-schema` SHALL print one schema to the standard output or
write one file per command into a directory. A key naming no command SHALL be
refused, so a renamed subcommand cannot leave a schema nobody can ask for.

#### Scenario: A notifier reads the sync payload from its schema
- GIVEN a build of neverest
- WHEN `neverest json-schema neverest-sync` runs
- THEN it prints the schema of the sync report, naming `conflicts` and `outstanding_conflicts` among its fields

#### Scenario: A checked account is one JSON document
- GIVEN a configured account
- WHEN `neverest check --json` runs
- THEN one document is printed, naming the account, its mode and every endpoint that answered

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
