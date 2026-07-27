---
cairn: change
change: wizard-on-bare-invocation
---

# Delta

## ADDED Requirements

### Requirement: A bare invocation runs the wizard
Running `neverest` with no subcommand SHALL run the configuration wizard
against the target configuration path (the first `--config` path when given,
else the default one), as a bare `himalaya` does. The command list SHALL stay
reachable through `--help`.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask
for confirmation before saving, SHALL ask again before overwriting an existing
file, and SHALL print the generated TOML document on stdout when either
confirmation is declined, so a generated configuration is never lost. In JSON
mode or when stdout is not a terminal, the wizard SHALL emit the document on
stdout without the save prompts, so `neverest > config.toml` and scripted runs
keep working.

A command that finds no configuration file SHALL propose the wizard ("No
configuration found, create one at `<path>`?") and SHALL exit when the proposal
is declined; the confirmation belongs to that proposal, not to the wizard, so a
bare invocation never asks it.

## MODIFIED Requirements

### Requirement: The wizard discovers in parallel and proposes what it found
The configuration wizard SHALL open with a welcome banner on stderr (what
neverest is, what the wizard does, where the documented sample lives), then ask
for **one** input: an email address (a bare domain is accepted and synthesized
as `@domain`; server URLs and folder paths are not wizard inputs). The account
name SHALL be derived from the email domain's first label, never prompted. The
banner SHALL be skipped in JSON mode.

Discovery SHALL run io-pim-discovery's parallel compose fan-out (fixed provider
rules, PACC, Mozilla autoconfig, RFC 6186 SRV) under a fan-out deadline, so one
unreachable endpoint cannot stall the wizard, and every reachable service SHALL
become one selectable entry carrying the authentication capabilities it
advertised; the concrete mechanism is picked after the entry is chosen. The
resolver SHALL be `NEVEREST_DNS_RESOLVER` when set, else the system resolver,
else a public default. Only backends compiled into the running build SHALL be
proposed, so the wizard never writes a side `client::open` refuses; when
discovery finds nothing, the wizard SHALL stop and point at the documented
sample rather than prompting for a hand-entered config.

IMAP credential prompts SHALL be narrowed by an unauthenticated CAPABILITY
probe, falling back to the discovery-advertised list when the probe fails, and
every secret SHALL be collected through the shared keyring / OAuth-broker
pickers. When discovery found a submission endpoint, the SMTP channel SHALL be
configured too, reusing the IMAP login and password on confirmation and
prompting a dedicated pair otherwise (neverest's SMTP channel authenticates
with LOGIN only, so reuse is offered only for a login + password mechanism; a
blank login means an unauthenticated relay). Every connection the wizard
configures SHALL be tested before the configuration is written.

## REMOVED Requirements

None.
