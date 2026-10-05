---
cairn: delta
change: batched-metadata-upgrades
---

## ADDED Requirements

### Requirement: Metadata probes are upgraded in bounded batches

The driver SHALL upgrade live Probed placements at the Meta tier in sequential batches of at most 64 handles. Each successful batch SHALL commit before the next fetch. A failure SHALL stop the upgrade without undoing earlier batches. A later run SHALL select only placements still Probed, excluding tombstones. Missing summaries SHALL remain eligible for a later run, without a same-run retry loop.

### Requirement: Metadata batches preserve duplicate identity order

The driver SHALL partition Meta probes in the lexical order of PimdirHandle, preserving the engine's existing duplicate-identity claimant order across batch boundaries. Metadata upgrades SHALL NOT change collection checkpoints or generations. Full-tier probes and their dry-run guard SHALL retain their existing behavior.

## MODIFIED Requirements

## REMOVED Requirements
