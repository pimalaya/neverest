---
cairn: delta
change: the-v1-review
---

# Delta

Folded into cairn/spec/sync.md phase by phase, as each lands.

## ADDED Requirements

### Requirement: `sync` initializes, queues or indexes on a flag
Phase 4: `--init`, `--queue-only` (the former `drain`) and `--index-only` (the former `index`); `drain` and `index` are no commands; their rows fold into the sync result under `queue` and `index`, `initialized` naming what `--init` initialized.

## MODIFIED Requirements

### Requirement: Text mode prints plain lines
Phase 2: replaces "The spinner reports in-mailbox progress" and "A spinner ends as what it reports": plain stderr lines, one a step, none under `--json`, no colour, no spinner, no crossterm.

### Requirement: A sync prints its progress as events
Phase 2: text mode prints the same events as plain stderr lines, the per-percent ones left out.

### Requirement: The configuration refuses every key it does not read
Phase 1: a key an earlier neverest read, `default` included, is refused as any unknown key is, by serde's `deny_unknown_fields`; no by-name sentence, no MIGRATION.md pointer.

### Requirement: A store the format outgrew is refused with its remedy
Phase 1: the refusal keeps its order (sync with the neverest that wrote it until nothing is queued, move the store away, `neverest init`, sync) and cites no MIGRATION.md, which is deleted.

## REMOVED Requirements

### Requirement: `default` is accepted and ignored
Phase 1: the key is refused as unknown (A8).
