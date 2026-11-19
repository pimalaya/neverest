//! # Run upkeep
//!
//! What a run does around its phases: the coverage, parked rows and conflicts
//! its report counts, the retention sweep and the replica a dry run uses.

use rusqlite::{Connection, OpenFlags};

use crate::sync::driver::*;

/// Reports what one endpoint holds of a collection it just synced (pimdir
/// SYNC §5): the last closed round's scope and when, and a round left open.
pub(super) fn report_coverage(
    store: &PimdirStore,
    collection: &str,
    ctx: &SourceCtx,
    report: &mut SyncOutput,
) {
    let rows = match store.list_coverage(collection) {
        Ok(rows) => rows,
        Err(err) => {
            warn!("cannot read the coverage of {collection}: {err}");
            return;
        }
    };

    for row in rows.into_iter().filter(|row| row.source == ctx.name) {
        let (since, until, at) = match row.coverage {
            Some(coverage) => (
                coverage.scope.since,
                coverage.scope.until,
                Some(coverage.at),
            ),
            None => (None, None, None),
        };
        report.coverage.push(CollectionCoverage {
            source: row.source,
            collection: display_name(&ctx.namespace, collection).to_owned(),
            since,
            until,
            closed_at: at,
            round: row.round.map(|round| OpenRound {
                since: round.scope.since,
                until: round.scope.until,
                started_at: round.started_at,
            }),
        });
    }
}

/// Surfaces the parked queue actions of the account's collections, once for
/// the run.
///
/// A parked row belongs to its collection, not a source: read per source, an
/// account syncing mail, contacts and calendar showed it three times. Every
/// run re-reports them; another account's rows are its own run's to report.
pub(super) fn report_parked(store: &PimdirStore, account: &str, report: &mut SyncOutput) {
    let owners = collection_accounts(store);
    match store.parked_actions() {
        Ok(parked) => {
            let parked = parked.into_iter().filter(|action| {
                owners
                    .get(&action.collection)
                    .is_some_and(|owner| owner == account)
            });
            for action in parked {
                report.parked.push(ParkedQueueAction {
                    id: action.id,
                    collection: action.collection,
                    kind: action.action,
                    producer: action.producer,
                    queued_at: action.created_at,
                    attempts: action.attempts,
                    code: None,
                    error: action.error,
                    sources: Vec::new(),
                });
            }
        }
        Err(err) => {
            warn!("cannot list parked actions: {err}");
            let err = pimdir_error(err).context("List the parked actions");
            report
                .failures
                .push(StepFailure::new(FailedStep::Queue, None, &err));
        }
    }
}

/// Counts the decisions the store is holding, once for the run.
///
/// Read from the store rather than from the run's own tally: the engine says
/// nothing about a placement it already parked, so a run itemizes only what it
/// newly marked, which is why that tally cannot count what is waiting.
pub(super) fn count_conflicts(store: &PimdirStore, account: &str, report: &mut SyncOutput) {
    match store.list_conflicts(Some(account)) {
        Ok(conflicts) => report.outstanding_conflicts = conflicts.len(),
        Err(err) => {
            warn!("cannot count the conflicts waiting for a decision: {err}");
            let err = pimdir_error(err).context("Count the conflicts");
            report
                .failures
                .push(StepFailure::new(FailedStep::Conflicts, None, &err));
        }
    }
}

/// Warns about the conflicts this run marked, one line per item.
///
/// Only what this run marked, which is the whole of the once-only rule: a
/// five-minute schedule over one unresolved card warns once, not three hundred
/// times a day. A `--json` caller notifies off `conflicts`, keeping no state.
pub(super) fn warn_conflicts(report: &SyncOutput) -> usize {
    for conflict in &report.conflicts {
        warn!("{conflict}");
    }

    report.conflicts.len()
}

/// Reclaims the account's retained items older than `store.purge-after`,
/// unset never.
///
/// Run after the sync, an item this run retired starts its delay, and it warns
/// instead of failing. A purge only releases a body (pimdir STORAGE §5), so
/// the collector runs after a purge that took something, whoever's it was.
pub(super) fn sweep_retained(
    account_config: &AccountConfig,
    store: &mut PimdirStore,
    report: &mut SyncOutput,
) {
    let Some(cutoff) = account_config.store.purge_cutoff(Utc::now()) else {
        return;
    };

    let items = purge_account(store, &cutoff);

    let collected = match items {
        0 => Default::default(),
        _ => match store.collect_garbage() {
            Ok(collected) => collected,
            Err(err) => {
                warn!("purged items but collected nothing: {err}");
                Default::default()
            }
        },
    };

    if items > 0 {
        info!(
            "purged {items} retained item(s) older than {cutoff}, collected {} object(s), {} byte(s) reclaimed",
            collected.objects, collected.bytes
        );
    }

    report.purged = Some(PurgedItems {
        items,
        objects: collected.objects,
        bytes: collected.bytes,
    });
}

/// Purges the retained items of the account `store` is opened for whose
/// retirement precedes `cutoff`, one at a time, answering how many.
///
/// Never another account's: the store's own sweep (`purge_retained_before`)
/// is store-wide, and one account's delay would decide for every account
/// sharing the store. A failure is warned about and skipped.
fn purge_account(store: &mut PimdirStore, cutoff: &str) -> usize {
    const PAGE: usize = 512;

    let account = store.account().map(str::to_owned);
    let collections = match store.list_collections_by_account(account.as_deref()) {
        Ok(collections) => collections,
        Err(err) => {
            warn!("cannot list the collections to sweep: {err}");
            return 0;
        }
    };

    let mut purged = 0;
    for collection in collections {
        let mut after = None;
        loop {
            let page = match store.list_retained(&collection.id, after, PAGE) {
                Ok(page) => page,
                Err(err) => {
                    warn!("cannot list the retained items of {}: {err}", collection.id);
                    break;
                }
            };
            let full = page.len() == PAGE;
            after = page.last().map(|item| item.seq);

            for item in page {
                let retired = item.retention.as_ref().and_then(|at| at.at.as_deref());
                if !retired.is_some_and(|at| at < cutoff) {
                    continue;
                }
                match store.purge(&collection.id, item.seq) {
                    Ok(true) => purged += 1,
                    Ok(false) => {}
                    Err(err) => warn!("cannot purge item {} of {}: {err}", item.seq, collection.id),
                }
            }

            if !full {
                break;
            }
        }
    }

    purged
}

/// The blob subdirectory of a pimdir store (pimdir STORAGE §5), shared by a
/// dry run.
pub(super) const BLOBS_DIR: &str = "objects";

/// A throwaway replica of the pimdir store, removed however the run ends.
///
/// Built beside the real store, not under the temporary directory, so the two
/// share a filesystem and the bodies are hardlinked rather than copied, which
/// on a tmpfs also saves gigabytes. Only the blob tree is ever shared.
pub(super) struct DryRunReplica {
    /// Where the replica lives, which is the run's `work_dir`.
    pub(super) dir: PathBuf,
}

impl DryRunReplica {
    /// Clones the store at `real_dir` into a sibling directory.
    ///
    /// An earlier run's leftover is cleared first, a panic aborting a release
    /// build with no destructor run. The replica is its user's alone (pimdir
    /// STORAGE §18), or its open refuses it as exposed.
    pub(super) fn new(real_dir: &Path) -> Result<Self> {
        let parent = real_dir.parent().unwrap_or(Path::new("."));
        let name = real_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("store"));
        let prefix = format!(".{name}-dry-");

        clear_stale_replicas(parent, &prefix);

        let dir = parent.join(format!("{prefix}{}", process::id()));
        modes::create_dir_all(&dir)
            .with_context(|| format!("Create dry-run replica {} error", dir.display()))?;

        if real_dir.exists() {
            let start = Instant::now();
            let mut counts = CloneCounts::default();
            let blobs = real_dir.join(BLOBS_DIR);

            let staging = modes::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(real_dir.join(STAGING_LOCK))
                .with_context(|| format!("Open the staging lock of {}", real_dir.display()))?;
            staging
                .lock_shared()
                .with_context(|| format!("Take the staging lock of {}", real_dir.display()))?;

            snapshot_database(real_dir, &dir).with_context(|| {
                format!("Copy the database of {} for dry-run", real_dir.display())
            })?;
            clone_dir(real_dir, &dir, &blobs, &mut counts)
                .with_context(|| format!("Clone {} for dry-run", real_dir.display()))?;
            drop(staging);

            debug!(
                "dry-run replica prepared in {:?} ({} linked, {} copied)",
                start.elapsed(),
                counts.linked,
                counts.copied
            );

            if counts.linked == 0 && counts.copied > 0 {
                info!(
                    "dry-run replica copied {} file(s), links unavailable",
                    counts.copied
                );
            }
        }

        Ok(Self { dir })
    }
}

impl Drop for DryRunReplica {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.dir) {
            warn!(
                "cannot remove the dry-run replica {}: {err}",
                self.dir.display()
            );
        }
    }
}

/// What [`clone_dir`] did, for the line reporting it.
#[derive(Default)]
struct CloneCounts {
    linked: usize,
    copied: usize,
}

/// Removes the replicas of earlier runs, named by `prefix` under `dir`.
///
/// A directory that will not go is left with a warning: it costs disk, never
/// correctness, the run about to start writing under a name of its own.
fn clear_stale_replicas(dir: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(prefix) {
            continue;
        }

        let stale = entry.path();
        match fs::remove_dir_all(&stale) {
            Ok(()) => warn!("removed a dry-run replica an earlier run left behind"),
            Err(err) => warn!("cannot remove {}: {err}", stale.display()),
        }
    }
}

/// The store's database, which a replica takes by [`snapshot_database`].
const DATABASE: &str = "pimdir.db";

/// The staging lock, held shared while a replica is taken (pimdir STORAGE
/// §8) so no collector unlinks a body the copied database names.
const STAGING_LOCK: &str = "objects.lock";

/// Copies the database of the store at `real_dir` into `dir` as one
/// consistent snapshot, through `VACUUM INTO` (pimdir STORAGE §8).
///
/// Producers write the store under no lock neverest holds, so a file copy of
/// the database and its WAL may tear between two of their commits.
fn snapshot_database(real_dir: &Path, dir: &Path) -> Result<()> {
    let real = real_dir.join(DATABASE);
    if !real.exists() {
        return Ok(());
    }

    let copy = dir.join(DATABASE);
    modes::options().write(true).create_new(true).open(&copy)?;
    let Some(target) = copy.to_str() else {
        bail!("Dry-run replica path {} is not UTF-8", copy.display());
    };

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(&real, flags)?;
    conn.execute("VACUUM INTO ?1", [target])?;
    Ok(())
}

/// Clones `src` into `dst`, hardlinking under `blobs` and copying the rest.
///
/// The database, taken by [`snapshot_database`], and the root's lock files,
/// holding no state, are left out. A link refused falls back to a copy:
/// misjudging a file costs a slower dry run, never a corrupted store.
fn clone_dir(src: &Path, dst: &Path, blobs: &Path, counts: &mut CloneCounts) -> Result<()> {
    modes::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().into_owned();
        let at_root = Some(src) == blobs.parent();

        if at_root && (name.starts_with(DATABASE) || name.ends_with(".lock")) {
            continue;
        }

        if entry.file_type()?.is_dir() {
            clone_dir(&from, &to, blobs, counts)?;
        } else if from.starts_with(blobs) && fs::hard_link(&from, &to).is_ok() {
            counts.linked += 1;
        } else {
            fs::copy(&from, &to)?;
            counts.copied += 1;
        }
    }

    Ok(())
}
