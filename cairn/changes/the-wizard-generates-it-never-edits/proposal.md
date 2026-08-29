---
cairn: change
id: the-wizard-generates-it-never-edits
status: landed
created: 2026-08-29
---

# `neverest configure` re-serialized the whole configuration file

## Why

Neverest was the one Pimalaya CLI whose `configure` edited an account instead of generating one. It resolved a name from `-a` or from the default account, re-ran discovery seeded with that account's current values, put the result back into the parsed `Config`, and called `Config::write`, which is `toml::to_string(self)` followed by `fs::write`. The whole document was re-serialized from the parsed model.

Everything the model does not carry is therefore destroyed: every comment, every blank line, every ordering choice, every hand-written formatting decision in the file. A configuration like the two-account one at `~/.neverestrc` is exactly the kind of file that loses the most, and it loses it on a single `neverest configure` that the user ran expecting to change one account.

The bare-invocation path had the same shape from the other end: the first-run wizard built a whole `Config`, offered to save it over the target, and asked "already exists, overwrite it?" when something was there. A yes there replaces the file rather than adding to it.

The seven other CLIs answer this with one contract, of which himalaya is the reference: read the existing file only for the names it takes and whether anything claims `default`, run the prompts, derive a free account name, claim the default only when nothing else does, and **append the rendered table as plain text**. A text append cannot lose what it never parsed.

## What changes

`configure` generates and never edits. It bails when stdin is not a terminal, reads the target for its taken names and default claim, runs discovery, derives a free name, claims the default only when no other account does, and hands back a `ConfigureOutput` carrying the rendered `[accounts.<name>]` table. JSON output and a redirected stdout print the document and touch no file; otherwise an existing file is appended to and a missing one written.

The cost is the seeding. Neverest's wizard pre-filled the email prompt from the account's current login, which the other seven never had because their accounts are one backend each while a neverest account names several sources. Editing an account is now the file's business and the user's editor's, against the documented sample.

## What does not change

Discovery itself: the single email prompt, the parallel fan-out, the capability-narrowed credential prompts, the connection test, and the one-account-one-source shape it writes. `sync` still returns its own `Exit`.
