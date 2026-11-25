---
cairn: log
change: docs-aligned-for-the-major
landed: 2026-11-25
---

# Aligned the docs on Ortie's for 1.0.0

## What landed

- README.md: Ortie's shape (header, a text `neverest sync` block, run by tests/docs.rs as the first sync of a fake CardDAV book, Features, install from pkg.pimalaya.org, Homebrew, installer, Cargo, Nix and sources, configuration template, provider recipes, numbered first run, Alternatives); tests/docs.rs checks the Features budget and the section order.
- CHANGELOG.md: the Unreleased section rewritten to changelog-001 and changelog-002, a lead paragraph for 1.0.0, one item per bullet, short bodies, churn folded into the final state.
- SECURITY.md: Ortie's text, 1.x supported.
- CONTRIBUTING.md: Ortie's reading order; sections restating the org guide removed, overlong paragraphs split.
- Capabilities moved: none (documentation only).
