---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 7: user-facing docs

- config.sample.toml rewritten on himalaya's shape (95 lines, `[accounts.example]` only, one trailing comment a key, enum values in brackets), tested per variant (`the_sample_configuration_parses_with_each_variant`) and shipped as share/doc/neverest/config.sample.toml by package.nix and install.sh.
- README cut to readme-017's budgets: eight Features bullets, Configuration with the arity table, Usage as command lines; the behaviour it explained lives in `--help` and the spec.
- CHANGELOG [Unreleased] rewritten as the net diff against 0.3.0 on io-imap's template, saying v1 reads no 0.x configuration or store.
- The change lands: the relay live cell (A2) and the README rule review (readme-017) are the owner's.
- Capability moved: none.
