---
cairn: delta
change: a-refused-line-exits-two
---

# Delta

## ADDED Requirements

### Requirement: A refused command line exits 2
A command line the grammar refuses (an unknown flag or subcommand, a missing or malformed argument) SHALL exit 2, clap's own status, through pimalaya-cli's `clap::parse`. With a `--json` word before any `--`, it SHALL print on stdout the failure object coded `usage-invalid`, its `error` clap's message without the `error: ` prefix and without styles, `sources` empty and `backtrace` null; without it, clap's rendering SHALL go to stderr. Help and version SHALL print and exit 0 as clap does.

1 stays a failure and the partial outcomes start at 3, so a script tells a mistyped invocation from a run that failed or did part of its work. The JSON schema of `neverest-error` SHALL admit `usage-invalid` beside neverest's own codes.

#### Scenario: An unknown flag under JSON
- GIVEN `neverest --json sync --no-such-flag`
- WHEN it runs
- THEN it prints one failure object coded `usage-invalid`, its `error` naming `--no-such-flag`, on stdout and exits 2

## MODIFIED Requirements

### Requirement: A conflicted run succeeds, with its own exit code
The conflict code SHALL be 3, the first partial-outcome status of the Pimalaya exit contract (pimalaya-cli's `EXIT_PARTIAL`); it was 2.

### Requirement: An incomplete run has its own exit code
The incomplete code SHALL be 4; it was 3.

### Requirement: `-a` filters the accounts a command runs over
A sync that left work waiting for a person makes the command exit 3, a failed or busy account 4.

### Requirement: A check reports every endpoint
A check with an endpoint that did not answer exits 3, where it exited 2.

### Requirement: A check opens the send channel too
A send channel that does not open makes the check exit 3, where it exited 2.

### Requirement: A divergence with no common ancestor parks rather than resolving itself
Its outcome names exit 3, where it named 2.

### Requirement: A refused write names the item as a person knows it
Its outcome names exit 3, where it named 2.

### Requirement: A send that may have been delivered is never sent again
Its outcome names exit 3, where it named 2.

### Requirement: A listing missing most of a collection waits for a person
Its outcome names exit 3, where it named 2.

### Requirement: A change over an unreadable body is withheld and reported
Its outcome names exit 3, where it named 2.

### Requirement: Neverest is the store's sole owner and drains the queue first
Its outcome names exit 4, where it named 3.

### Requirement: A drain applies the queue without syncing
Its outcome names exit 4, where it named 3.

### Requirement: A run holds the store lock, waiting bounded
Its outcome names exit 4, where it named 3.

### Requirement: A quota refusal keeps the send queued
Its outcome names exit 4, where it named 3.

### Requirement: An unreached endpoint is named in the report
Its outcome names exit 4, where it named 3.

### Requirement: A throttled provider is waited out, then named
Its outcome names exit 4, where it named 3.

### Requirement: A push no server answered is pushed again
Its outcome names exit 4, where it named 3.

### Requirement: The store's owner builds its search index
Its outcome names exit 4, where it named 3.

## REMOVED Requirements
