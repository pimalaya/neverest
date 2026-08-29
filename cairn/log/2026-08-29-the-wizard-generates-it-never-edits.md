---
cairn: log
change: the-wizard-generates-it-never-edits
landed: 2026-08-29
---

# The wizard generates, it never edits

Neverest was the last Pimalaya CLI whose `configure` edited an account, and the way it saved destroyed the file it edited. It resolved a name from `-a` or from the default account, re-ran discovery seeded with that account's current values, put the result back into the parsed `Config`, and called `Config::write`: `toml::to_string(self)` followed by `fs::write`. The whole document came back out of the parsed model, so every comment, blank line, ordering choice and hand-written formatting decision was gone. A two-account hand-written `~/.neverestrc` loses all of it on one `neverest configure`.

The first-run path failed the same way from the other end: it built a whole `Config`, offered to save it over the target, and asked "already exists, overwrite it?" when something was there. Answering yes replaced the file rather than adding to it.

## What landed

**[src/cli/configure.rs](../../src/cli/configure.rs) is the contract the other seven CLIs already run**, himalaya being the reference. It bails when stdin is not a terminal, naming config.sample.toml as the way out. It reads the target file for the two things it constrains, the account names already taken and whether anything claims `default`, and for nothing else. It runs discovery, derives a free name by suffixing until the configuration does not hold it, and claims the default only when no other account does. The result is a `ConfigureOutput` carrying the rendered table, the name and the default claim.

`--json` and a redirected stdout print the document and touch no file, which is what makes `neverest configure > config.toml` work. Otherwise a missing file is written and an existing one is **appended to as plain text**, `"\n{document}"` through `OpenOptions::append`. Nothing is parsed and serialized back, so there is nothing left to destroy.

**[AccountConfig::render](../../src/config.rs)** renders one `[accounts.<name>]` table: groups ordered with the backend first and the sync options last, a blank line between them, and `server` or `user-id` lifted to the top of its group ahead of the credential authenticating against it. An account naming several sources renders under the same single header, its sources being dotted keys, so an appended table never opens a header a later account would fall into.

**`discover::run` returns the proposed name beside the account** and nothing else. What becomes of that account is the command's business.

**Deleted:** `src/wizard/edit.rs`, `Config::write` (its only two callers were the wizard's two save paths) and `AccountConfig::direct_sources` (only the edit path read it). `offer_configuration` and `print_welcome` moved beside the command, and the dispatcher stopped handing `configure` an account.

`ConfigureOutput` is registered as `neverest-configure` in the schema registry, and no command reports through `Message` any more.

## What was given up

The prompts were seeded with the account's current values, so re-running `configure` over `posteo` pre-filled the email from its IMAP login. That is gone with the edit path. The other seven never had it because their accounts are one backend each, while a neverest account names several sources and the seeding only ever covered the first direct one. Changing an account is now the file's business and the user's editor's, against config.sample.toml.

## Not changed

Discovery: the single email prompt, the parallel fan-out, the capability-narrowed credential prompts, the connection test, and the one-account-one-source shape it writes. `sync` still returns its own `Exit`.

## Capabilities moved

- wizard: generation replaces editing; the derived free name, the single default claim, the text append
- config: `AccountConfig::render`, the account-table renderer; `Config::write` removed
- output: `configure` prints a `ConfigureOutput` instead of a message
