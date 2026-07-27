---
cairn: tasks
change: wizard-on-bare-invocation
---

# Tasks

- [x] `cli/main`: make the subcommand optional (`Option<Command>`).
- [x] `main`: dispatch a bare invocation to the wizard, against
      `Config::target_path`.
- [x] `wizard/discover`: `run` takes the printer, drops the "no configuration
      found" prompt, and ends on save-or-print (save confirmation, overwrite
      guard, stdout fallback) instead of an unconditional write.
- [x] `wizard/discover`: emit the config straight to stdout in JSON mode or
      when stdout is redirected.
- [x] `config`: `load_or_wizard` takes the printer, proposes the wizard
      ("No configuration found, create one at ...?") and exits when declined.
- [x] `cli/{check,init,sync,configure}`: pass the printer to `load_or_wizard`.
- [x] `cli/{check,configure}`: give both commands their own `--help` summary
      (they were showing the flattened account flag's doc).
- [x] Docs: README, CHANGELOG, wizard module docs.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`, feature subsets, and
      a bare run showing the banner.
