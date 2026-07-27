---
cairn: log
change: wizard-on-bare-invocation
landed: 2026-08-07
---

# A bare `neverest` runs the wizard

The wizard had no deliberate entry point: it only ran when a command found no
configuration file, so `neverest` alone was a clap error ("requires a
subcommand"). The subcommand is now optional and a bare invocation runs the
wizard against the target config path, as a bare `himalaya` does; `--help`
still lists the commands.

**Entry point** (`cli/main.rs`, `main.rs`): `Cli::command` becomes
`Option<Command>`, and `None` dispatches to `wizard::discover::run` with
`Config::target_path(config_paths)` (the first `--config` path, else the
default XDG one). Neverest has no global account flag, so there is no
half-typed `--account` case to send to the help, unlike Himalaya.

**Save or print** (`wizard/discover.rs`): the wizard ended on an unconditional
`config.write(target)`, which was only safe while it could not run over an
existing file. It now ends on `save_or_print`: a save confirmation, then an
overwrite confirmation when the target exists, and the generated TOML printed
on stdout (through the printer, as a `GeneratedConfig` wrapper rendering TOML
in text mode and the config object in JSON mode) when either is declined, so
the result is never lost. JSON mode and a redirected stdout skip the prompts
entirely and emit the document straight away, so `neverest > config.toml` and
scripted runs work like Himalaya's and Ortie's. The banner is skipped in JSON
mode.

**The proposal moved to the caller** (`config.rs`): the "No configuration
found, create one at `<path>`?" confirmation (declining exits) was a step
inside the wizard, which made it wrong for a bare run over an existing config.
It now lives in `Config::load_or_wizard`, whose job it is; the wizard opens on
its banner from both entry points. `load_or_wizard` takes the printer, threaded
from the four commands that call it (`check`, `init`, `sync`, `configure`).

**Help text** (`cli/check.rs`, `cli/configure.rs`): both commands had no doc
comment, so clap fell back on the flattened `AccountFlag` doc and listed them
as "The account name flag parser". Each now carries its own summary.

Verified: bare `neverest -c <path>` opens on the banner and the email prompt;
42 tests green; fmt clean; clippy clean except the pre-existing
`incompatible_msrv` warning in `cli/sync.rs`; feature subsets (none / imap /
msgraph / imap+smtp, each over `rustls-ring`) compile warning-free.

Spec updated: `sync` (ADDED: "A bare invocation runs the wizard"; MODIFIED:
"The wizard discovers in parallel and proposes what it found" now states the
banner is skipped in JSON mode).
