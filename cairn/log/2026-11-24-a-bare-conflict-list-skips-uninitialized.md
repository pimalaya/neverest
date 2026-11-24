---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A bare conflict list skips the accounts not initialized

Task N14 of the-live-night-findings. On the live night of 2026-11-23, a bare `neverest conflict list` failed on the first account of the rc no `init` had run over, reading no other. Reproduced on 2026-11-24 with a scratch rc holding the initialized Stalwart account and an uninitialized `afresh`:

```
Error: Account afresh not initialized, run `neverest init -a afresh` or `neverest sync --init -a afresh` first
exit 1
```

A listing naming no account (src/cli/conflict.rs) now skips such an account and names it under the new `uninitialized` key of `ConflictListOutput`:

```
No item is waiting for a decision

Not initialized (1), not read:
 - afresh, run `neverest init -a afresh` first
exit 0
```

JSON: `{"conflicts":[],"uninitialized":["afresh"]}`. Naming it with `-a afresh` still fails coded `account-uninitialized`. Capability moved: sync.md, "An unknown conflict and an uninitialized account are coded".

## Checked

- tests/accounts.rs `a_bare_conflict_list_skips_the_accounts_not_initialized`: with the old behaviour it fails (`account-uninitialized`, exit 1), with the fix it passes.
