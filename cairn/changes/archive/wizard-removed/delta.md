---
cairn: delta
change: wizard-removed
---

# Delta

## ADDED Requirements

### Requirement: A bare invocation prints the help
Running `neverest` with no subcommand SHALL print the help and exit 0, with a configuration file or without one: neverest writes no configuration, so a first run has nothing to offer but the commands. A configuration is written by hand against the documented sample, config.sample.toml.

A command that finds no configuration file SHALL fail, exit 1, naming the path it looked at (the first `--config` path when given, else the default one) and the documented sample; it SHALL NOT exit reporting success. A configuration file that fails to parse SHALL be an error rather than read as absent.

## MODIFIED Requirements

### Requirement: A backend owns its ALPN default
Neverest SHALL NOT restate a backend's default in the config schema; nothing else writes one.

### Requirement: A collection records the account that syncs it
The example of two accounts holding one source name is the `imap` sugar, no longer the wizard's source.

### Requirement: The default store root resolves on every supported platform
The reason no longer cites the wizard reaching a configuration before the store.

### Requirement: Microsoft Graph is a first-class source
The scopes a token needs per domain are named by config.sample.toml, no longer by the wizard.

### Requirement: A data command describes what it prints
`configure` no longer returns data; the registry check's bare `neverest` shows the help only.

## REMOVED Requirements

All with the wizard (pimalaya-cli 0.4 removed the prompts and sign-in helpers they stood on):

- The wizard configures one channel from the account's credentials
- The wizard discovers in parallel and proposes what it found
- Each domain gets its best option without a question
- The review lists each pick in plain words
- Nothing discovered asks for a server URL
- A typed server URL names its protocol by its scheme
- The sign-in mechanism is picked, not asked
- Sign-ins are grouped by identity
- A password is typed once, tested, then stored
- OAuth hands off to Ortie
- A refusal offers a way on
- The collections to sync are picked last
- The wizard generates an account and never edits one
- The generated configuration is a dotted document
- A bare invocation offers the wizard only on a first run (replaced by "A bare invocation prints the help")
