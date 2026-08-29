---
cairn: log
change: data-commands-describe-what-they-print
landed: 2026-08-29
---

# Data commands describe what they print

`--json` was an interface with no shape written down. The README tells a reader to pipe `neverest sync --json` into `jq` and test two fields; everything else about the payload had to be learned by running the command and looking at what that particular run happened to fill. And two commands were not printing a payload at all: `check` said its piece in two messages, `init` in one, so `check --json` emitted two JSON documents and `init --json` a single object whose only field was prose.

## What landed

`SyncReport` is `SyncOutput`, and it, its hunks and the `Flag` they carry derive `JsonSchema` alongside `Serialize`. The conflict verbs already named their outputs and now derive it too. `check` reports a `CheckOutput`: the account, its mode, and one entry per endpoint it opened with the number of collections that endpoint listed. `init` reports an `InitOutput`: the account, its mode, the store directory it created and the endpoints it opened.

`src/json_schema.rs` is the registry, one entry per data command keyed by its invocation path, and `neverest json-schema` (aliased `json-schemas`) prints one schema to the standard output or writes one file per command with `--dir`. A test walks the clap parser and refuses a key naming no command, so renaming a subcommand cannot leave a schema nobody can ask for.

## Not changed

`sync` still returns its own `Exit`. A run that reconciled everything and still parked something is neither a success nor a failure, and the code that says so reads off the report before the printer consumes it, which is exactly where it was.

`configure` keeps its `Message`. It confirms that an account was configured and reports nothing about it, which is what a message is for. The text output of `check` and `init` says what it said before; only the number of writes to the printer changed.

## Capabilities moved

- **sync**: a data command now owes a named output type and a registered schema, and a message is reserved for confirmations.
