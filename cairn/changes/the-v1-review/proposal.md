---
cairn: change
id: the-v1-review
status: active
created: 2026-11-16
---

# The v1 review

Owner's manual review of neverest, 2026-11-16, compacted for implementation. Baseline: master f84c242 plus the owner's staged Cargo.toml (`[dev-dependencies]` moved above `[dependencies]`) and deny.toml (RUSTSEC-2023-0071 ignore, `domain` git source and `0BSD` dropped; `cargo deny check` passes), which ship with phase 1.

## Decisions (owner, 2026-11-16)

- D1: next release is v1.0.0; Cargo.toml version bumped by the owner when ready. Lifts the "stay on 0.x" rule.
- D2: no migration from 0.x to v1 (new config, resync); migration guides start at v1 to v2. Applies to cardamum, calendula, carillon at their v1.
- D3 (F1): unknown and removed keys are refused by serde `deny_unknown_fields` alone; every `removed_key!` by-name refusal goes, family-wide.
- D4 (F3): plain terminal output, no colors, no spinner, no crossterm; JSON is the interface; questions read through std stdin on every OS, yes/no only.
- D5 (F4): AI writes no inline `//` comment; `///` and `//!` short per the guidelines; `// SAFETY:` MUST above every `unsafe`, MAY above panicking code; humans may write inline comments.
- D6: `sync` gains `--init` (initialize what is not, then sync), `--queue-only` (old `drain`), `--index-only` (old `index`); `drain` and `index` commands removed; `init` command kept; no `--yes`, no question in `sync` this round; `--dry-run` stays the preview.
- D7 (F2): READMEs get shorter family-wide, neverest first.

## Assumed accepted (proposed in review, not objected; veto before phase 0)

- A1: offline/ folds into sync/; driver.rs splits into sync/driver/ by phase.
- A2: src/offline/pipe.rs replaced by `std::io::pipe` (MSRV 1.89 has it; 64 KiB kernel buffer instead of 256 KiB, owner checks the relay live cell).
- A3: src/offline/prof.rs and `NEVEREST_PROFILE` deleted.
- A4: `submit` keeps its name (RFC 6409, the family's queued action kind).
- A5: item/ dissolves to src/collection.rs and src/flag.rs; kind/ keeps mod.rs and mail.rs; kind/merge.rs moves to src/conflict/merge.rs; kind/event.rs moves to src/calendar.rs.
- A6: the three backend factorings of phase 3, and the suspected gcal/gpeople dead-stream bug confirmed by test before any fix.
- A7: size rule proposal for GUIDELINES.md: a function SHOULD stay under 80 lines, a file under 1000 lines of non-test code (owner sets the numbers); doc and README ceilings checked by a test in tests/docs.rs.
- A8: `default = true` refused as an unknown key rather than read and warned.
- A9: inline-comment enforcement is a Claude Code hook (below), not a repo check; existing `//` are all removed but `SAFETY`, the owner re-adds any by hand.

## Guidelines (pimalaya/.github, with phases 0, 5, 6, 7)

- inline-004 rewritten per D5 (AI writes no `//`; `SAFETY` MUST on unsafe, MAY on panics; humans may comment); inline-008 says where a why goes; AI_POLICY.md gains the line for contributors using AI.
- New size rule per A7; readme rules per phase 7; the audit names tests/docs.rs as the check.

## Family follow-ups (after neverest lands, not in this change)

- F1: `removed_key!` out of himalaya, cardamum, calendula, carillon, ortie, then out of pimalaya-config (src/toml.rs:326, test :685).
- D2: delete MIGRATION.md in cardamum, calendula, carillon; reword their refusals.
- F3: pimalaya-cli drops `spinner`, rewrites `prompt` on std (`bool` as `[y/N]` read_line refusing off a terminal and naming `--yes`, `item` as a numbered list), drops inquire and crossterm; then himalaya, cardamum, calendula, tcal, tcard, mml.
- F4: purge `//` in io-pimdir (876), himalaya (486), io-imap (428), calendula (271), carillon (214), cardamum (206), ortie (66), cli (45), comodoro (38), config (21).
- F2: READMEs after the v1 releases: himalaya 531 lines, calendula 349, cardamum 301, himalaya-tui 237, comodoro 227, ortie 222, carillon 186.
- Later: TLS, SASL and proxy config types shared in pimalaya-config (ten copies today; the orphan rule turns the impls into free functions).

## Parked

- Questions at sync gates on a terminal (`--init`, `--accept-mode`, `--accept-absences` asked `[y/N]`, their flags answering off a terminal): later round.
