---
cairn: change
id: wizard-on-bare-invocation
status: landed
created: 2026-08-07
---

# A bare `neverest` runs the wizard, like a bare `himalaya`

## Why

The wizard landed with `discovery-wizard` but has no way to be run on purpose:
its only entry points are `Config::load_or_wizard` (reached when a command
finds no configuration file) and `neverest configure` (which needs an account
to already exist). A bare `neverest` is a clap error, because the subcommand is
mandatory:

```
$ neverest
error: 'neverest' requires a subcommand but one was not provided
```

Himalaya, whose flow this wizard mirrors, treats a bare invocation as the
first-run wizard: with no account there is nothing else useful to do, and the
command list is one `--help` away. Neverest should behave the same, so
discovering the wizard does not depend on running a command that happens to
fail first.

## What (design)

- **The subcommand becomes optional.** `neverest <command>` is unchanged;
  `neverest` with no subcommand runs the wizard against the target config path
  (the first `--config` path, else the default XDG one). `neverest --help`
  still lists the commands.
- **The wizard is no longer write-only.** It ran unconditionally through
  `config.write(target)`, which was safe when it could only run with no
  configuration on disk. A bare invocation can now happen over an existing
  file, so the write is guarded: the wizard asks before saving, asks again
  before overwriting an existing file, and prints the generated TOML on stdout
  when either answer is no, so the result is never lost.
- **Non-interactive stdout stays non-interactive.** In JSON mode or when
  stdout is redirected (`neverest > config.toml`), the document is emitted
  straight to stdout without the save prompts, as in Himalaya and Ortie.
- **The "no configuration found" confirmation moves out of the wizard** into
  `Config::load_or_wizard`, where it belongs: it is the proposal made by a
  command that found no config (declining exits), not a step of the wizard
  itself. A bare `neverest` therefore never asks it, and the wizard opens on
  its banner in both entry points. `load_or_wizard` takes the printer so the
  wizard can emit its document through it.

## Out of scope

- Exiting after the wizard on the `load_or_wizard` path: a command that
  proposed the wizard keeps running against the configuration it just built,
  as it already did.
- Any change to what the wizard asks or writes.
