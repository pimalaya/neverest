---
cairn: change
change: both-endpoints-report-what-they-parked
---

# Delta

## MODIFIED Requirements

### Requirement: Conflicts are surfaced in the run report
A placement the engine marked `conflicted` SHALL appear in the sync report (text
and `--json`), naming its collection and item, and SHALL keep appearing on every
run until it is resolved. This SHALL hold whatever the account's topology is: a
source reconciled against the store alone and a source reconciled against a
target report a parked divergence the same way.

A run SHALL name a divergence once. A collection is reconciled until it is
quiescent, so a pass runs several times over it and every endpoint reports into
one report; a divergence the run's own merge settles and a later pass marks again
is one divergence, one line and one notification. A create a side refused and a
write a side would not take SHALL likewise be named once per item, however many
passes met the same answer, while two copies of one identity stay two refusals.

A run SHALL first merge the three bodies and resolve the conflict where the merge
reports no collision, so only a genuine disagreement is surfaced. Neverest SHALL
NOT decide a collision by itself; that decision is an edit, staged through the
pimdir queue by whoever owns it.

#### Scenario: A mirrored pair names the endpoint that parked it
- GIVEN an account naming one source and one target
- WHEN a run parks a divergence on either of them
- THEN the report names the item, its collection and that endpoint, and the notification is raised

#### Scenario: A convergence loop does not multiply the report
- GIVEN a collection whose reconcile takes several passes
- WHEN a divergence the run settled is marked again by a later pass
- THEN it is reported once
