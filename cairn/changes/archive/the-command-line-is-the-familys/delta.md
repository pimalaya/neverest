---
cairn: delta
change: the-command-line-is-the-familys
---

# Delta

## ADDED Requirements

### Requirement: A first run meets the family's welcome
A command finding no configuration file, a bare `neverest` included, SHALL fail with pimalaya-config's missing-configuration error, exit 1: "No configuration found at <path>, start from the sample: <url>", the path being the first `-c` path when given, else the default one, and the URL config.sample.toml's. When the output is text and stderr is a terminal, a person SHALL meet pimalaya-cli's first-run welcome on stderr in its place, still exiting 1: the commands creating the file from the sample a package ships, or downloading it when none ships, then `neverest check`. A script or a `--json` caller SHALL get the error.

With a configuration, a bare `neverest` SHALL print the help and exit 0, a configuration that fails to parse counting as one, its error left to the first command reading it. A command reading a configuration that fails to parse SHALL fail with that error rather than read it as absent. Neverest writes no configuration.

A configuration holding no account SHALL fail pointing at the sample the same way.

#### Scenario: A first run
- GIVEN no configuration file
- WHEN `neverest sync` runs from a script
- THEN it exits 1 naming the path it looked at and the sample, and writes nothing

#### Scenario: A first run at a terminal
- GIVEN no configuration file
- WHEN a person runs `neverest`
- THEN the welcome prints on stderr, giving the commands that create the file and `neverest check`, and the run exits 1

### Requirement: The command line is the family's
`-c/--config` SHALL be pimalaya-cli's shared flag: one path per flag, never split, repeatable, the first file the base and the next ones deep-merged on top in order. No environment variable SHALL name the configuration (`NEVEREST_CONFIG` is not read). `-a/--account` SHALL stay repeatable, unlike the other Pimalaya CLIs, where it names the one account a command runs on: a neverest run covers several accounts.

`completion`, `manual` and `json-schema` SHALL come in that order, with pimalaya-cli's abouts, and SHALL run like any other command when given no argument: what is ambiguous to write on the standard output fails with the failure object on stdout, exit 1. The help SHALL end with pimalaya-cli's footer. clap SHALL be built with the feature set the family uses (`derive`, `error-context`, `help`, `std`, `suggestions`, `usage`), so the help and errors are plain text read from no environment.

Neverest SHALL have no `repl` and no `mcp`: its commands are batch operations over every account and its store (sync, drain, check, init, index), which a session or an agent gains nothing from composing; reading and writing items is the frontends' job (himalaya, cardamum, calendula), each with its own `repl` and `mcp` over the store neverest keeps.

#### Scenario: Two configuration files
- GIVEN `-c public.toml -c private.toml`, each declaring one account
- WHEN `neverest drain` runs
- THEN it runs over both accounts, the second file merged on top of the first

#### Scenario: A path holding a colon
- GIVEN `-c 'a:b.toml'`, a file that does not exist
- WHEN `neverest check` runs
- THEN it fails naming `a:b.toml` whole

## MODIFIED Requirements

### Requirement: `-a` filters the accounts a command runs over
`-a/--account` SHALL be repeatable and SHALL filter the configured accounts. Without it, `sync`, `check`, `drain` and `init` SHALL run over every configured account, store by store and in name order within a store, one after the other inside one run holding each store's lock once; with it, over the named ones. The configuration is merged into a sorted table before neverest reads it, so name order is the only order it can keep. A name the configuration does not declare SHALL fail the command before anything runs, coded `account-missing`, in the family's words: "Account `x` not found, the configuration holds: a, b" (`default` among them, a name like any other: neverest has no default account); and so SHALL an account whose configuration does not validate, the configuration being one document.

Each account's failure SHALL be its own: the run SHALL go on with the next account and SHALL end with one result listing every account's outcome, a failed one carrying its error and its `code`. The command SHALL exit 0 when every account succeeded, otherwise with the partial-outcome code its own outcomes give (3 for a sync that left work waiting for a person), a failed or a busy account making it 4, the code of a run that could not do all its work. A failure common to the run (no configuration, a store refused as a whole) SHALL fail the command with the failure object, exit 1.

`index` is store-wide: it SHALL build the stores of the accounts the filter keeps, each once and whole, every account of a store included. `conflict show`, `conflict resolve` and `sync --reset` address one item or start one account over, so they SHALL need exactly one account: a filter leaving none or several SHALL fail, naming the accounts. `conflict list` SHALL list the conflicts of every account it runs over, each row naming its account.

#### Scenario: Two accounts, one of them failing
- **GIVEN** two accounts, the first initialized and the second not
- **WHEN** `neverest sync --json` runs
- **THEN** the first is synced, the result lists both, the second with its error naming `neverest init -a <account>`, and the run exits 4

#### Scenario: An unknown account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync -a perso` runs
- **THEN** it exits 1 before anything runs, coded `account-missing`, its error "Account `perso` not found, the configuration holds: home, work"

#### Scenario: A reset needs one account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync --reset` runs with no `-a`
- **THEN** it exits 1, naming both accounts, and resets nothing

## REMOVED Requirements

### Requirement: A bare invocation prints the help
Replaced by "A first run meets the family's welcome": a bare `neverest` without a configuration now fails as any command does.
