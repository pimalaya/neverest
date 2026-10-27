---
cairn: log
change: machine-output-contract
date: 2026-10-27
---

# Adopted the shared machine output contract

pimalaya-cli is pinned at `55b6693` until 0.3.0. `json-schema` gained `neverest-error`, the schema of the JSON failure object, and each schema now carries `$id` = `https://pimalaya.org/schemas/neverest/v0.3/<key>.json`. A test asserts every payload root is an object; all already were, so no output moved. Piped `--json` is now one value per line, from the pimalaya-cli printer.

Verified: 384 binary tests pass with all features (integration suites too), clippy clean on all features and targets, `--no-default-features` builds. Exit codes 2 and 3 are unchanged.

Spec updated: cairn/spec/sync.md (ADDED: The JSON contract is described and versioned; MODIFIED: A data command describes what it prints).
