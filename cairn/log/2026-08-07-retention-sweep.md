---
cairn: log
change: retention-sweep
landed: 2026-08-07
---

# Neverest sweeps the store's retained items

The pimdir store stopped deleting items: when an item's last binding vanishes
(a remote expunge, propagated) the row is *retained*, hidden from the sync seam
and from listings but kept with its body. Retention with no reclamation is a
leak, and reclaiming is a schedule rather than a semantic, so it belongs to a
client. Neverest is that client on desktop: it holds the store lock for the
whole run, it is the store's sole owner, and it runs on the user's cadence.

**Config** (`config.rs`): `store.purge-after`, a `HumanDuration` newtype parsing
one integer plus a unit (`s`, `m`, `h`, `d` = 86400 s, `w` = 7 d) and rendering
back the largest unit that divides evenly, so a value round-trips through the
document. Unset means never purge, `"0"` purges immediately (the old terminal
delete). No boolean beside it: the delay is the switch, so a configuration
cannot spell a contradiction. Months and years are refused, having no fixed
length; this is a retention delay, not calendar arithmetic.

`StoreConfig::purge_cutoff(now)` returns `now - purge-after` as RFC 3339 with
millisecond precision and a `Z`, the exact shape the store stamps `retained_at`
with, so the store's comparison is a plain lexicographic one on equally shaped
instants. io-pimdir stays clock-free: the cutoff is the caller's parameter.

**Driver** (`offline/driver.rs`): `sweep_retained` calls
`purge_retained_before(cutoff)` **after** the sync and before the report is
finalised, on both run paths (two-side and single-source), never in a dry run.
Running it after the sync means an item this run retired starts its delay now
rather than being reclaimed by the very run that retired it. It warns rather
than fails, in the style of the send channel: a store that cannot be swept is a
housekeeping problem, not a reason to fail a run that synced correctly.
`sync --no-purge` skips it for one run.

**Report** (`sync/report.rs`): a `purged` section (`PurgedItems { items, bytes
}`) in `--json`, rendered as a `Retention:` block in the text output when
anything was actually reclaimed, so a quiet run stays quiet.

**Docs**: `config.sample.toml` documents the knob and the backup recipe;
README gains a "Retention and purging" section. The same docs pass fixed two
stale items: the CHANGELOG advertised an m2dir `soft-delete` option under Added
(the m2dir side was removed on 2026-08-01, so the feature does not exist in this
release, and retention is what actually delivers what it promised), and the
README documented a `left.m2dir.*` example plus an `init` writing `state.json`
into the cache directory (it creates `pimdir.db` in the state directory).

Verified against io-pimdir's working tree (the retention surface landed the
same day, so neverest carries a `[patch.crates-io]` on it until it publishes):
56 unit tests green, clippy clean across every feature subset (`rustls-ring`
alone and with each of `imap` / `smtp` / `msgraph`), except the pre-existing
`incompatible_msrv` warning in `cli/sync.rs`.

Spec updated: `sync` (ADDED: "A run reclaims retained items on a schedule").
