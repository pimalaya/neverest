---
cairn: delta
change: the-wizard-generates-it-never-edits
---

## ADDED Requirements

### Requirement: The wizard generates an account and never edits one
`neverest configure` SHALL generate a new account. It SHALL NOT read an account back, seed the prompts with its values, or write it out again: editing an account, adding a second by hand, and everything the prompts do not cover belong to the file and the user's editor, against the documented sample.

`configure` SHALL take no account: `-a` names an account to run against, and there is nothing to name when the wizard generates. The dispatcher SHALL NOT hand it one.

The account name SHALL be derived and never prompted, being only the table key, and it SHALL be free: the wizard SHALL suffix the name discovery proposes (`posteo`, `posteo-2`, …) until the configuration does not already hold it. A second `[accounts.<name>]` table makes the whole document fail to parse, taking the accounts that used to work down with it.

The generated account SHALL claim `default` only when no account already in the configuration does. Two `default = true` accounts would make the one every command picks depend on map ordering.

A configuration file that fails to parse SHALL be an error rather than read as absent: appending to a broken document buries the real problem under a second one.

## MODIFIED Requirements

### Requirement: A bare invocation offers the wizard only on a first run
Running `neverest` with no subcommand SHALL print the help, except on a machine
with no configuration, where it SHALL offer the wizard first, as a bare
`himalaya` does. The wizard targets the first `--config` path when given, else
the default one. A configuration that fails to parse counts as present, so the
offer never proposes to write over a broken file; the parse error surfaces when
a real command reads it. A declined offer SHALL fall back to the help, a bare
invocation having nothing else to run.

The offer SHALL be skipped, and the help printed, in JSON mode and when stdin
is not a terminal, neither being able to answer a prompt. It SHALL also be
skipped when `--account` names an account: with no subcommand that is a
half-typed command rather than a first run, and the help is what points at the
commands.

`neverest configure` itself SHALL refuse to run when stdin is not a terminal,
naming the documented sample as the way out: a wizard cannot prompt a cron job.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask
for confirmation before saving to a path holding no file, SHALL ask before
appending to one that does ("Append account `<name>` to `<path>`?"), and SHALL
print the generated TOML document on stdout when either confirmation is
declined, so a generated account is never lost. In JSON mode or when stdout is
not a terminal, the wizard SHALL emit the document on stdout and touch no file,
so `neverest configure > config.toml` and scripted runs keep working.

Appending SHALL be a plain text append of `"\n<document>"` to the file opened
in append mode. The wizard SHALL NOT parse a configuration file and serialize
it back: comments, ordering and hand-written formatting are not in the parsed
model, and re-serializing destroys every one of them.

A saved account SHALL be reported on stderr, naming the file it landed in and
the name it landed under, since that name was never asked for; an account that
did not claim the default SHALL be told it is reachable through `-a <name>`.

A command that finds no configuration file SHALL propose the wizard, under the
same two guards, and SHALL then read the configuration again rather than trust
the wizard's result, which is only printed when the save is declined. A command
still finding none SHALL fail naming the path it looked at and the documented
sample; it SHALL NOT exit reporting success.

### Requirement: The wizard discovers in parallel and proposes what it found
The discovery fan-out already resolves CalDAV and CardDAV services alongside
IMAP and submission, and the wizard SHALL offer every reachable service whose
backend is compiled into the running build, not only the mail ones. A run that
finds services of several kinds SHALL offer them as separate entries, one per
kind.

The wizard SHALL write **one account with one source**, the offline replica,
which is the common case and the only one worth automating. Everything beyond
it, a second kind, a mirror, a fan-in, is configured by hand against
config.sample.toml. The picked service is written through the direct-backend
sugar (`imap.server = …`), and an account with no target retains every body and
reads offline with no further setting. The wizard SHALL NOT write `one-way`,
`retain` or a `targets` table: their defaults are the offline replica it exists
to produce.

All other wizard rules (the single email-address prompt, the fan-out deadline,
the capability-narrowed credential prompts, the connection test before writing)
are unchanged.

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one
`[accounts.<name>]` table header per account, the only header in the document,
with every field below it written as a dotted key. An empty table SHALL write
nothing. The saved file and the document printed on stdout SHALL be identical.

A rendered account SHALL be readable in that order rather than the serializer's:
the groups SHALL be ordered with the backend the wizard wrote before the sync
options it never writes, each group SHALL be separated by a blank line, and the
key naming what a group points at (`server`, `user-id`) SHALL be lifted to the
top of its own, ahead of the credential authenticating against it.

An account naming several sources SHALL render under that same single header,
its sources being dotted keys like every other field, so appending a table
after it never opens a header a later account would fall into.

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type
deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema`
for the registry, and SHALL print it once. It SHALL NOT report data as a
message: a message serialises as one prose string, so `--json` over one yields
nothing a consumer can read, and several messages in one run yield several
documents where a consumer expects one value.

`configure` returns data: the account it generated, under the name and default
claim it derived. Its `Display` is the TOML document alone, which is what makes
a redirected stdout a usable configuration file.

Every such output type SHALL be registered in the schema registry under its
invocation path, the command path joined with hyphens and prefixed `neverest-`,
and `neverest json-schema` SHALL print one schema to the standard output or
write one file per command into a directory. A key naming no command SHALL be
refused, so a renamed subcommand cannot leave a schema nobody can ask for.

## REMOVED Requirements

None.
