---
cairn: change
id: a-check-reports-every-endpoint
status: landed
created: 2026-11-03
---

# A check reports every endpoint

## Why

`neverest check` stopped at the first endpoint or SMTP channel that did not answer and exited 1 with the failure object. A frontend adding an account learned that one thing was wrong, never which others were fine, and fixing it led to the next failure one run at a time. pimalaya-cli's output contract (every-output-is-data) asks the opposite of every binary: a `check` with a failed backend prints every row, `{backend, ok, code, error}`, and exits 2. The 2026-11-03 review missed it for neverest; himalaya's `account check` is the reference.

## What

- Every endpoint is checked whatever the others answer, rows sorted by name (they followed a hash map's order before).
- An endpoint row is `{source, backend, ok, code, error, collections, capabilities, smtp}`: the shared check row plus neverest's diagnostics. `backend` is the protocol table (`imap`, `caldav`, `msgraph-calendar`…). A failed row lists no collection and keeps the capabilities, which come from the configuration.
- `smtp` is the channel's own check row, `{backend: "smtp", ok, code, error}`, `null` when the endpoint declares none, when its credentials did not resolve, or in a build without SMTP. It was a bool.
- A credential that does not resolve fails its endpoint's row only.
- A failed endpoint or channel prints the report and exits 2 (`PartialOutcome`); text mode marks each failed row `FAIL` and ends on "has endpoints that did not answer".

## Compatibility

Breaking: `check` exits 2, not 1, when an endpoint fails, and prints the report rather than the failure object; `smtp` is an object or `null`, not a bool. Lands in the next release with the sync events.
