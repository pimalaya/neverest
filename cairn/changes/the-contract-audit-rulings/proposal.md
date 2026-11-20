---
cairn: change
id: the-contract-audit-rulings
status: landed
created: 2026-11-20
---

# The contract audit rulings

The CLI contract audit of 2026-11-20 (ledger ~/.claude/plans/pimalaya-family-ledgers/cli-contract-audit-2026-11-20, report reports/neverest.md, findings NEV-X1 to NEV-X14) found neverest's outputs diverging from pimalaya-cli's contract where neverest's own spec had ruled otherwise, and a handful of defects. The owner ruled on 2026-11-20 ("Go for all"): neverest follows the kit rather than the kit gaining neverest exceptions. This reverses part of the 2026-11-17 naming rulings (the named failure lists) and narrows the N6 ruling on progress lines.

## Rulings applied

- A1: an account-level `tls` table, inherited field by field by every endpoint's and `smtp`'s `tls`, himalaya's keys; the check rows fill pimalaya-cli's `tls`.
- A2.1 (NEV-X5): the sync report's conflict row prints the `id` `conflict show` and `conflict resolve` take and the store `collection` `conflict list` prints, the handle as `remoteId`; `conflict list` renames `handle` to `remoteId`.
- A2.2 (NEV-X6): `conflict show` bodies are `{contents, encoding}`, `utf-8` or `base64`, `null` when absent.
- A2.3 (NEV-X7): the failure rows of a sync report move into one `errors` list of pimalaya-cli item errors, each tagged `kind` (the list it was in) with its own keys; lists reporting what happened or a state stay. Parked rows become the kit's queue rows. Exit 3 for a person, 4 for a rerun, kept.
- A2.4 (NEV-X8): progress lines reach a pipe for a full `sync` alone.
- A2.5 (NEV-X9, NEV-X14): `account check` prints pimalaya-cli's account check, a list of accounts, rows under `backends` with neverest's keys after the shared ones, `mode` per account, `orphans` at the top.
- NEV-X1: the shutdown is installed before `--init` and the lock wait; a signal there prints the result, exit 4.
- NEV-X3: `--queue-only` keeps the store's code on a malformed row and fails the store's accounts, not the command.
- NEV-X4: `--queue-only` reports every parked row of the account's collections, exit 3, as `sync`.
- NEV-X10: `conflict-not-found` and `account-uninitialized` codes, and `item-duplicate` on a refused duplicate.
- NEV-X11: an aborted interactive resolve exits 3.
- NEV-X12: a bare `neverest --json` goes through pimalaya-cli's `meet_bare_invocation`, exit 2.
- NEV-X13: imperative abouts, the mode sentence's agreement, `1 collection`, the sync help naming a signal among the exit-4 causes, conflict text without a leading blank line.

## Not done

- NEV-X2 (a first signal cancels in-flight reads): a blocked read sits inside pimalaya-stream's retry loop and the protocol clients' own streams, which expose no cancellation; neverest cannot reach the socket. Needs a cancellation hook in pimalaya-stream (a shared shutdown flag its `Retry::Until` loop polls between short socket timeouts), then the unreached row for the hung source.

## Breaking

Every JSON shape change above is breaking; neverest is 0.x going to 1.0, so no migration guide (the 0.x to v1 rule), CHANGELOG lines only.
