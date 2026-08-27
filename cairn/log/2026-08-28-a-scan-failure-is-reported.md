---
cairn: log
change: a-scan-failure-is-reported
date: 2026-08-28
---

# A scan failure is reported, with the error that caused it

`sync -d -s carddav` said `already in sync` over an address book holding cards.
It was not in sync: the only collection had failed to enumerate, and two lines
between the failure and the operator hid it.

`phase1_spine` caught the failed spine, logged a warning and moved on, leaving
the report empty, so the run printed the one sentence that cannot be true of a
collection it never managed to look at. A failed scan is now a `PatchEntry`
carrying its error, so the run reports it and exits non-zero. The warning stays
for the log.

The error itself was truncated. `Remote enumerate error: {err}` renders an
`anyhow` error with `Display`, which prints the outermost context and drops
every source beneath it, so the backend's HTTP status and body summary, kept
verbatim precisely so a caller can read them, never arrived. Finding the status
meant reading a TLS trace log for something the error already held. The
enumerate, fetch and push wrappers now render with `{err:#}`.

This is the diagnosis apparatus, not the diagnosis. Which side is at fault when
a server refuses a request is a separate question, and answering it needs the
evidence this change stops throwing away.

Capabilities moved: **sync** (the run report).
