---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A dry run initializes nothing

Task N8 of the-live-night-findings. On the live night of 2026-11-23, `sync --init -d` initialized every account it covered, so a dry run created the store and stamped the sidecar. Reproduced on 2026-11-24 on Stalwart with a scratch store: `neverest -a stalwart sync --init -d -m INBOX` printed "Created the store" and left pimdir.db behind.

Under `--dry-run`, `--init` now initializes nothing (src/cli/init.rs `uninitialized`): the result names the accounts it would initialize under `initialized`, and `driver::run` adds each to its dry-run replica alone (`initializing`), creating the replica's store when there was none, then plans the sync there. A store that does not exist is not locked, nothing being there to lock. Capability moved: sync.md, the `--init` rule of `neverest sync`.

## Checked

- tests/accounts.rs `sync_init_dry_run_initializes_nothing`: without the fix the sidecar gains the account ("the dry run initialized an account"); with it the sidecar is unchanged and no store is created.
- Live on Stalwart (scratch store): the same command now prints "Account stalwart would apply 58 hunks", `initialized: ["stalwart"]`, `dryRun: true`, and leaves no store.
