---
cairn: log
change: bare-invocation-help
landed: 2026-08-26
---

# A bare `neverest` prints the help unless there is nothing configured

The spec claimed a bare invocation runs the wizard "as a bare `himalaya` does",
and `main::execute`'s `None` arm did just that, calling `discover::run`
unconditionally. Himalaya does the opposite: `meet_bare_invocation` loads the
configuration first and offers the wizard only when it finds none. So a machine
with a `~/.neverestrc` in place got the welcome banner and the email prompt,
targeting `~/.config/neverest/config.toml` rather than the file it had just
failed to notice, and then offering to save over it; the command list, which is
what someone with a working configuration is after, never appeared.

**`main::meet_bare_invocation`** (new) is Himalaya's, function for function: the
configuration is loaded with `from_paths_or_default(...).ok().flatten()`, so a
file that exists but fails to parse counts as configured and the offer never
proposes to write over a broken one. The offer is raised only when nothing is
configured, no `--account` was given, the output is not JSON and stdin is a
terminal; everything else, a declined offer included, falls through to
`Cli::command().print_help()`.

**`discover::offer_configuration`** (new) holds the proposal itself (the
prompt, then `run`), and reports whether the wizard ran. It is the single place
the wizard introduces itself to someone who did not ask for it, shared by the
bare invocation and by the offer a command raises.

**`Config::load_or_wizard`** was prompting unconditionally, so a cron job and a
`--json` consumer both met a prompt they cannot answer and failed on the TTY
rather than on the missing configuration; declining called `exit(0)`, reporting
success for a run that did nothing. It now guards the offer on the same two
conditions, re-reads the configuration afterwards rather than trusting the
wizard's return value (the wizard prints the document instead of saving it
whenever the save is declined), and fails naming the path it looked at and
`config.sample.toml`, the way Himalaya's `resolve_account` does. `process::exit`
is gone from the crate.

Verified: `neverest` with `~/.neverestrc` in place prints the help;
`neverest sync -c /nonexistent/config.toml` off a terminal fails with "No
configuration found at /nonexistent/config.toml, run `neverest` to generate one
or write it by hand: …". Tests green, fmt and clippy clean.

Spec updated: `sync` (MODIFIED: "A bare invocation runs the wizard" is now "A
bare invocation offers the wizard only on a first run").
