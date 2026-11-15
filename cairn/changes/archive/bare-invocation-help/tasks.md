---
cairn: tasks
change: bare-invocation-help
---

- [x] `meet_bare_invocation` prints the help when a configuration is found
- [x] The bare offer is guarded on `--json`, a non-terminal stdin and `--account`
- [x] A declined bare offer falls back to the help
- [x] `offer_configuration` holds the proposal, shared by both entry points
- [x] `load_or_wizard` skips the offer in JSON mode and off a terminal
- [x] `load_or_wizard` fails naming the path and the sample instead of `exit(0)`
- [x] `load_or_wizard` re-reads the configuration after the wizard ran
- [x] Docs: the module headers and the `--help` text no longer promise a wizard
