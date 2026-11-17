---
cairn: log
change: text-progress-on-stdout
date: 2026-11-17
---

# Text progress on stdout

Line N6 of the CLI family analysis of 2026-11-15, ruled by the owner on 2026-11-17: neverest follows pimalaya-cli's output spec.

## What landed

- Every text-mode progress line (the sync events, `check`, `init`, the credential resolution and the index build) is printed on stdout, flushed as it comes, before the result, where it went to stderr. Text mode is read by a person, who reads one stream; stderr stays the logs'; a script reads `--json`, one event a line and the result alone on the last.
- No change under `--json`.

## Spec

sync.md: "Text mode prints plain lines" and the text events sentence.
