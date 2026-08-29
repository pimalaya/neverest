---
cairn: change
id: data-commands-describe-what-they-print
status: landed
created: 2026-08-29
---

# `--json` had no shape written down, and two commands had no shape at all

## Why

Neverest offers `--json` and documents it as the interface a notifier reads: the README tells the reader to pipe `neverest sync --json` into `jq` and test `conflicts` and `outstanding_conflicts`. Nothing said what else is in there. A consumer learned the payload by running the command and looking, which means it learned the fields a particular run happened to fill and none of the ones it did not, and a field that moved broke it silently. Every other Pimalaya CLI answers this with a `json-schema` command over a registry of output types; neverest derived `JsonSchema` nowhere and shipped no registry.

Two commands were worse than undescribed. `check` printed the account's mode as one message, then a healthy line as another; `init` printed one message. Under `--json` that is two JSON documents on the standard output for `check`, which no parser reads as one value, and for `init` a single `{"message": "…"}` whose only field is prose. The rule the family follows is that `Message` carries confirmations and never data, and both were carrying data.

## What

- Every data command hands the printer a named `*Output` type deriving `Display`, `Serialize` and `JsonSchema`. `SyncReport` is renamed `SyncOutput`, the conflict verbs already had theirs, and `check` and `init` gain `CheckOutput` and `InitOutput` in place of their messages.
- `src/json_schema.rs` maps each command's invocation path (`neverest-sync`, `neverest-check`, `neverest-init`, `neverest-conflict-list`, `neverest-conflict-show`, `neverest-conflict-resolve`) to the schema of what it prints, and the `json-schema` command, aliased `json-schemas`, writes them out.
- `configure` keeps its `Message`: it confirms, it does not report.
