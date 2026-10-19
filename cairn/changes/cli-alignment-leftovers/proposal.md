---
cairn: change
id: cli-alignment-leftovers
status: active
created: 2026-10-18
---

# What neverest still owes the family alignment

## Why

The cross-repo alignment audit of 2026-10-18 (CLI_ALIGNMENT_PLAN.md, which kept the item ids of the 2026-08-28/29 plan) found these items still open in neverest. Each was re-checked against the code on 2026-10-18.

- **F2**: an unknown or missing account fails with a bare "Cannot find account" (src/cli/drain.rs:49, src/cli/sync.rs:179, src/cli/init.rs:38, src/cli/conflict.rs:310, and src/cli/check.rs:44), where the family names the accounts held and the two ways to pick a default.
- **F7**: no `footer!` on the clap command, which the rest of the family carries.
- **H3**: src/config.rs:982-992 hand-rolls `shell_expanded_path_opt`, and its own TODO waits for the shared twin. pimalaya-config 0.2.1 ships it as `opt_shell_expanded_path`; Cargo.lock is still on 0.2.0.
- **A2**: the `[patch.crates-io]` git pins wait for releases.

pimalaya-cli will take over a local copy once its wizard-save-layer change lands: `ExistingConfig`, `append_or_print` and `print_saved` (src/cli/configure.rs:100, :253, :279). Its shared-account-resolution change is the natural home for the F2 wording, so F2 may land as adopting it rather than as five local fixes.

## What

Close each item above, one task each.
