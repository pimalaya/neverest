---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 6: comments and doc ceilings

- Every `//` comment line is gone from src/ and tests/ (about 880 lines; no `unsafe` exists, so no `// SAFETY:` was needed). Narration went; a real why moved, compressed, into the item's `///` or the module's `//!`, or into the spec (*The rules the code's comments carried*).
- Doc comments trimmed to inline-001 and inline-002 (about 650 blocks); src/cli/ keeps clap's help, src/main.rs every header section.
- tests/docs.rs checks the ceilings over src/ (a `//!` paragraph of four lines, a `///` summary of two lines at most plus three) and the README's Features and section order.
- Not settled: about 150 `///` blocks keep a two-line summary, which the test allows and a strict one-line reading of inline-002 does not; the "TODO: item-level sync filters" roadmap note left with the comments.
- Capability moved: sync (the rules the code's comments carried).
