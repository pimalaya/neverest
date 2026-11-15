---
cairn: change
id: bare-invocation-help
status: landed
created: 2026-08-26
---

# A bare `neverest` on a configured machine ran the wizard

## Why

The spec said a bare invocation runs the wizard "as a bare `himalaya` does", and the code did exactly that: `main::execute`'s `None` arm called `discover::run` unconditionally. Himalaya does not do that. Its `meet_bare_invocation` loads the configuration first and only offers the wizard when it finds none; an existing configuration gets the help, and so does a JSON caller, a non-terminal stdin, and a `--account` that names an account with no subcommand to run it.

The result on a configured machine: `neverest` with a `~/.neverestrc` in place opened the welcome banner and the email prompt, targeting `~/.config/neverest/config.toml` rather than the file that already existed, and offering to save over it. Nothing pointed at the command list, which is what someone with a working configuration typing the bare name is looking for.

The offer a command raises when it finds no configuration diverged too. `Config::load_or_wizard` prompted unconditionally, so a cron job and a `--json` consumer both hit a prompt they cannot answer and failed on the TTY rather than on the missing configuration; declining called `exit(0)`, which reports success for a run that did nothing.

## What

- `main::meet_bare_invocation` mirrors Himalaya's: it offers the wizard only when the configuration is missing, the output is human, stdin is a terminal and no `--account` was given, and falls back to `--help` in every other case, including a declined offer. A file that exists but fails to parse counts as configured, so the offer never proposes to write over a broken one.
- `discover::offer_configuration` is the one place the wizard introduces itself to someone who did not ask for it, shared by the bare invocation and by `load_or_wizard`.
- `load_or_wizard` guards the offer on the same two conditions, and replaces `exit(0)` with a failure naming the path it looked at and the documented sample, as Himalaya's `resolve_account` does. It re-reads the configuration after the wizard rather than trusting its return value, since the wizard prints the document instead of saving it whenever the user declines.
