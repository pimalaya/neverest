---
cairn: change
id: one-way-overwrites-a-diverged-target
status: active
created: 2026-10-02
---

# A one-way account overwrites a diverged target

Self-contained: a session with no prior context can take it from here. Reported as https://github.com/pimalaya/neverest/issues/30.

## Why

"`one-way` declares authority, not silence" says a difference resolves in the source's favour, no conflict recorded. It does not: the source and the target editing one field differently parks a conflict on the target, and the run exits 2.

Found 2026-10-02 by tests/endpoints.rs `a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence`, against Radicale (`./tests/radicale.sh`). The same failure on io-pimdir 0.5.0 and 0.5.1, so it predates 0.5.1's "a conflict never hides a server edit". The second run of the test reports:

```json
{"conflicts":[{"side":"b","collection":"authority","id":"card-1.vcf"}],"outstandingConflicts":1}
```

Nothing is lost, the target keeping its own body, but the mode's one promise is broken: a user who declared an authority gets asked anyway.

## What

Find where the target's divergence becomes a conflict despite `Authority::Store`, whose policy is `PimdirConflictPolicy::PreferLocal` (src/offline/driver.rs, `Authority::conflict_policy`), and make the run overwrite the target in that same run.

Leads, unverified:

- The source's edit and the target's edit meet as an item-level conflict (the shared item flagged, pimdir SYNC §3) rather than a binding one, and io-pimdir's `PimdirSync` returns on `local.status == Conflict` before the policy is read (io-pimdir src/sync.rs, around the `if local.status == PimdirStatus::Conflict` branch).
- Or the hub marks the cross-endpoint divergence before the target's round runs, the way "A divergence with no common ancestor parks" does on purpose, without the `one-way` exception that requirement states ("Under `one-way` nothing parks").

Decide which layer owns the fix: neverest not marking under `one-way`, or io-pimdir honouring the policy on an item conflict. The second touches a conformance vector, so prefer the first if it suffices.

## Not in scope

Changing what `one-way` means. The spec already says what it should do.
