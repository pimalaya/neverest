---
cairn: log
change: text-output-is-defused
date: 2026-11-17
---

# Text output is defused

Finding NEV-13 of the CLI family analysis of 2026-11-15: neverest never called the sanitizer, so `conflict show` printed stored bodies raw, events and hunks printed collection names, ids and flags raw, and draft-13 added the server's reason for a refusal, where cardamum and calendula defuse the same bytes.

## What landed

- src/terminal.rs: `TerminalPrinter` wraps the stdout printer in `main`, every text result and event displayed through `Defused`, which runs `pimalaya_cli::sanitize::text` over each line, line breaks and tabs kept, `\r` before a break dropped; JSON serializes the inner value unchanged. src/notice.rs passes its stderr lines through the same `terminal::text`.
- Test `a_line_keeps_its_breaks_and_loses_its_escapes`.
- One choke point rather than a call per field: a new row cannot reach the terminal raw.
- Trivial fix: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *Text mode prints plain lines* amended.
