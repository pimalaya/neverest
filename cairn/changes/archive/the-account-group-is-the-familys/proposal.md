---
cairn: change
id: the-account-group-is-the-familys
status: landed
created: 2026-11-15
---

# The account group is the family's

## Why

Line N4 of the CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15; audit finding F1, decision D1, "yes to account group, needs to be aligned with the rest"): himalaya, cardamum and calendula carry `account list`, `account check`, `account capability`; neverest had a top-level `check` and no `account list`. pimalaya-cli 0.5.1 ships the shared outputs (`pimalaya_cli::account`) and its spec requires the group, a former top-level `check` kept as a hidden alias.

## What

- pimalaya-cli 0.5.1, then 0.5.2 (`cargo update -p pimalaya-cli`, no other package moved).
- `account check`: today's `check`, its report unchanged; `check` a hidden alias; both schema keys.
- `account list`: `AccountListOutput` rows. `backends` are the backend tables the account writes; `notBuilt` those the build leaves out.
- The check's send channel row is pimalaya-cli's `BackendCheck<ErrorCode>` in place of neverest's identical `ChannelCheck`, and every check row is coded through `error::coded`, reaching a server refusal's code.
- The welcome names `account check`.

## Decisions

- **The check keeps its shape.** pimalaya-cli's `AccountCheckOutput` is one account and its backends; neverest's check covers every account `-a` leaves, with each account's mode, every endpoint's collections, roles and capabilities, and the orphan accounts of each store. Its rows already lead with the shared check keys. Breaking the report for the shared type would lose the doctor's content.
- **`backends` lists protocol tables, not sources.** The row's type is `&'static str`, the family's backend names; a neverest account names its sources itself (`sources.work.imap`), which `account check` reports. Every endpoint counts, targets included (they are opened and written to), each protocol once, and `smtp` for a send channel, which `account check` opens too. It is read off the tables alone, so an account a command would refuse (two senders, a bad arity) still lists.
- **JMAP is `notBuilt`**: a JMAP table parses and every build refuses to open it.
- **`default` is `true` on every account**: the row says whether a command with no `-a` runs against the account, which in neverest is every account; the `default` key stays ignored.
- **`-a` filters `account list`** as it filters every neverest command, an unknown name failing coded `account-missing`.
- **No `account capability`**: the capabilities each source declares are in `account check`, read from the configuration; a separate command would print a subset of it.
