---
cairn: delta
change: bare-invocation-help
---

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A bare invocation offers the wizard only on a first run
Running `neverest` with no subcommand SHALL print the help, except on a machine
with no configuration, where it SHALL offer the wizard first, as a bare
`himalaya` does. A configuration that fails to parse counts as present, so the
offer never proposes to write over a broken file; the parse error surfaces when
a real command reads it. A declined offer SHALL fall back to the help, a bare
invocation having nothing else to run.

The offer SHALL be skipped, and the help printed, in JSON mode and when stdin
is not a terminal, neither being able to answer a prompt. It SHALL also be
skipped when `--account` names an account: with no subcommand that is a
half-typed command rather than a first run, and the help is what points at the
commands.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask
for confirmation before saving, SHALL ask again before overwriting an existing
file, and SHALL print the generated TOML document on stdout when either
confirmation is declined, so a generated configuration is never lost. In JSON
mode or when stdout is not a terminal, the wizard SHALL emit the document on
stdout without the save prompts, so `neverest > config.toml` and scripted runs
keep working.

A command that finds no configuration file SHALL propose the wizard ("No
configuration found, create one at `<path>`?"), under the same two guards, and
SHALL then read the configuration again rather than trust the wizard's result,
which is only printed when the save is declined. A command still finding none
SHALL fail naming the path it looked at and the documented sample; it SHALL NOT
exit reporting success.

#### Scenario: A configured machine gets the command list
- GIVEN a configuration file at one of the default paths
- WHEN `neverest` runs with no subcommand
- THEN the help is printed and no prompt is raised

#### Scenario: A scripted command names what is missing
- GIVEN no configuration file and a stdin that is not a terminal
- WHEN `neverest sync` runs
- THEN it fails naming the target path and the sample configuration, with no prompt

## REMOVED Requirements

None.
