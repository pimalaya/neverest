---
cairn: change
id: machine-output-contract
status: landed
created: 2026-10-27
---

# Adopt the shared machine output contract

## Why

pimalaya-cli now owns the machine contract every binary prints (cli `machine-output-contract`, commit `55b6693`): a schema for the failure object, schemas stamped with the contract version, piped `--json` delimited by newlines, and a check that every payload is a JSON object (what an MCP client takes as structured output). Himalaya adopted it first; Neverest follows, without error codes since it has none.

## What

- The JSON Schema registry returns a `JsonSchemaRegistry` (bin `neverest`, the crate version), registers `neverest-error` as pimalaya-cli's `ErrorReport`, and tests that no payload root is anything but an object.
- Errors are still evaluated through the uncoded `ErrorReport::eval`.
- pimalaya-cli is pinned at `55b6693` until its 0.3.0 release.

No payload changes: every registered root already is an object.
