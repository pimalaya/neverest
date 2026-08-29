---
cairn: delta
change: conflicts-merge-before-they-park
---

## ADDED Requirements

### Requirement: A run merges what nobody disagreed about
A run SHALL three-way merge the base, local and diverging bodies of a marked conflict, and SHALL resolve it as an ordinary edit when the merge reports no collision. The merge SHALL be built in rather than configured, and SHALL be dispatched on the collection's kind: vcard-rs for contacts, ical-rs for calendars and tasks and journals. Mail is immutable-content and reaches none of this.

It SHALL resolve on an empty report and on nothing else. Being unswappable is what forces that: a merge nobody can replace has no business deciding anything a person might have decided differently, and the report distinguishes the two exactly.

Most divergence is not disagreement. Two sides editing different fields of one card have said nothing contradictory, and the stored base is what proves it, by naming which side touched which field. Reporting those to a person is a background tool asking to be switched off.

#### Scenario: Disjoint edits need no one
- GIVEN a conflicted contact whose sides changed different fields
- WHEN the run merges it
- THEN both changes survive, the conflict clears through the queue, and nothing is reported

#### Scenario: A collision is not merged away
- GIVEN a conflicted contact whose sides set the same field differently
- WHEN the run merges it
- THEN the conflict stays parked and the run reports it

### Requirement: A conflicted run succeeds, with its own exit code
A run that reconciled its collections and left conflicts behind SHALL exit with a code distinct from both success and failure, and SHALL report the outstanding count read from the store rather than the count the run itself marked.

A conflict is one item wide. Failing the run would stop every other item over one divergence, and under a supervisor restarting on failure it would loop over a state no supervisor can resolve. The distinct code says the same thing without pretending the run broke.

The two counts differ and the difference matters: the engine emits nothing for a placement already parked, which is what keeps notifications quiet across repeated runs, and which is also why the run's own tally is not the number of decisions waiting.

#### Scenario: A parked conflict does not fail the run
- GIVEN a collection holding one parked conflict beside ordinary items
- WHEN it is synced
- THEN the ordinary items reconcile, the run exits with the conflict code, and the outstanding count is reported

### Requirement: Entering a conflict notifies once
A run SHALL notify when a placement enters conflict, through the configured notification, defaulting to a log entry and never shelling out unasked. A run observing a conflict already parked SHALL notify nothing.

An unattended tool that repeats itself is one a user silences. A five-minute schedule and one unresolved conflict is otherwise nearly three hundred notifications a day, all naming the same card.

#### Scenario: The second run is quiet
- GIVEN a conflict marked by one run and left unresolved
- WHEN a later run observes it again
- THEN nothing is notified

### Requirement: Deciding is a command, never a run
Neverest SHALL NOT decide a content collision during a sync, and SHALL NOT open an editor or any interactive program from one, whatever is attached to its terminal. Deciding SHALL be `neverest conflict resolve` and nothing else.

`--prefer-local` and `--prefer-remote` discard a side, which is what a person may ask for by name and what a background run may never do on its own. `--interactive` SHALL hand the bodies to the configured merger as filesystem paths, base first, then the divergent sides, then the path to write, and SHALL take the result only on a zero exit with that path modified. A non-zero exit, or an untouched output, SHALL leave the conflict exactly as it was: an editor exits zero on a bare quit, and reading that as a choice would discard a side by accident.

A tty is not consent. A run has one when it is driven by a wrapper script, when it is watched from a pane nobody is sitting at, and when it is a person waiting, and the three are indistinguishable from inside. Escalating on that signal blocks every remaining collection behind a human who may not exist.

#### Scenario: A sync with a terminal still parks
- GIVEN a run attached to a tty that marks a collision
- WHEN the run continues
- THEN no program is spawned, the conflict parks, and the remaining collections reconcile

#### Scenario: An aborted merger changes nothing
- GIVEN an interactive resolution whose merger exits non-zero, or leaves its output untouched
- WHEN it returns
- THEN the conflict is unchanged and nothing is pushed

### Requirement: A resolution is refused when the remote moved under it
`neverest conflict resolve` SHALL record the revision the resolution was computed against and SHALL refuse to push when the store has since observed a newer one, reporting it rather than applying it.

An unresolved conflict tracks the newest remote revision on every run, so a decision made in an editor over an hour can be a decision about a version nobody holds any more. Pushing it would overwrite everything that arrived meanwhile, which is the loss the parking exists to prevent, arriving at the last step instead of the first.

#### Scenario: A stale decision is not applied
- GIVEN a resolution computed against one revision
- WHEN the store has observed a newer one before it is applied
- THEN the push is refused and the conflict is reported as moved

## MODIFIED Requirements

### Requirement: Conflicts are surfaced in the run report
Unchanged in what it requires of the report: a placement the engine marked `conflicted` SHALL appear in the sync report (text and `--json`), naming its collection and item, and SHALL keep appearing on every run until it is resolved.

What changes is what reaches it. A run SHALL first merge the three bodies and resolve the conflict where the merge reports no collision, so only a genuine disagreement is surfaced. Neverest SHALL NOT decide a collision by itself; that decision is an edit, staged through the pimdir queue by whoever owns it.

## REMOVED Requirements

None.
