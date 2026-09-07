---
cairn: delta
change: the-merger-is-configured-once
---

# Delta

## ADDED Requirements

### Requirement: The merger is named once for the document
The interactive merger SHALL be configurable at the document level as well as under an account, the account's value winning where it is set and the document's applying where it is not. Neither SHALL be required, and an account naming no merger with no document-level one SHALL leave `conflict resolve --interactive` with nothing to run, as it does today.

The merger is a property of the person rather than of the account: the same program settles a collision whichever account raised it, the way one `$EDITOR` serves every repository. Requiring it per account copies one long line, carrying four placeholders in a fixed order, once per account.

The two SHALL be reconciled where the account configuration is loaded, so every reader still sees one optional command and no call site learns there are two places it could have come from.

An `$EDITOR` or `$VISUAL` fallback SHALL NOT be inferred: a merger is handed four paths under a contract no text editor knows, so guessing one would open the base file and read a bare save as a decision.

#### Scenario: One merger serves every account
- GIVEN a document naming `conflict.merger` and two accounts naming none
- WHEN a conflict is resolved interactively under either
- THEN both hand the bodies to that merger

#### Scenario: An account overrides the document
- GIVEN a document naming `conflict.merger` and an account naming another
- WHEN a conflict is resolved interactively under that account
- THEN the account's merger runs and the document's does not

## MODIFIED Requirements

### Requirement: Deciding is a command, never a run
Neverest SHALL NOT decide a content collision during a sync, and SHALL NOT open an editor or any interactive program from one, whatever is attached to its terminal. Deciding SHALL be `neverest conflict resolve` and nothing else.

`--prefer-local` and `--prefer-remote` discard a side, which is what a person may ask for by name and what a background run may never do on its own. `--interactive` SHALL hand the bodies to the configured merger as filesystem paths, base first, then the divergent sides, then the path to write, and SHALL take the result only on a zero exit with that path modified. A non-zero exit, or an untouched output, SHALL leave the conflict exactly as it was: an editor exits zero on a bare quit, and reading that as a choice would discard a side by accident.

A command naming any of `{base}`, `{local}`, `{remote}` and `{output}` SHALL be substituted rather than appended, for a tool whose output is a flag rather than its last argument. The documentation SHALL show the form each named tool actually takes: tcard and tcal both take three positionals and their output as `--output`, so both are the placeholder form, and neither runs when handed four positionals.

A tty is not consent. A run has one when it is driven by a wrapper script, when it is watched from a pane nobody is sitting at, and when it is a person waiting, and the three are indistinguishable from inside. Escalating on that signal blocks every remaining collection behind a human who may not exist.

#### Scenario: A sync with a terminal still parks
- GIVEN a run attached to a tty that marks a collision
- WHEN the run continues
- THEN no program is spawned, the conflict parks, and the remaining collections reconcile

#### Scenario: An aborted merger changes nothing
- GIVEN an interactive resolution whose merger exits non-zero, or leaves its output untouched
- WHEN it returns
- THEN the conflict is unchanged and nothing is pushed
