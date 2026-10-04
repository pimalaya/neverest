---
cairn: log
change: an-incomplete-run-has-its-own-exit-code
date: 2026-10-04
---

# An incomplete run has its own exit code

A sync whose server refused the connection reported the scan error and exited 0, the code of a run with nothing to do; with a conflict waiting it exited 2. MOA could not tell offline from idle without parsing the error strings of the report.

## What landed

- src/sync/report.rs: `SyncOutput::incomplete`, true when a collection or item hunk carries an error, or a send or an intent failed without parking.
- src/cli/exit.rs: `Exit::Incomplete`, exit code 3, winning over the conflict code.
- Checked against Stalwart: the same account exits 0 online and 3 with its IMAP port closed.

## Capabilities moved

- sync: an incomplete run has its own exit code.
