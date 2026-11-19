//! # Driver tests
//!
//! The driver's runs over scripted remotes and temporary stores.

use io_pimdir::{
    change::{PimdirChange, PimdirChangeKind, PimdirDropReason},
    client::{producer::PimdirEnqueued, reader::PimdirReader},
    collection::{PimdirCheckpoint, PimdirCursor},
    hub::PimdirHubConflict,
    object::PimdirObject,
    placement::PimdirOrigin,
    remote::{
        PimdirEnumerate, PimdirEnumerated, PimdirFetchedBody, PimdirListing, PimdirPushOutcome,
        PimdirPushResult, PimdirRemote, PimdirRemoteItem, PimdirRemoteMeta, PimdirRemoteSnapshot,
    },
    summary::mail::PimdirMailSummary,
};

use super::*;
use crate::{conflict::merger::SPAWNS, sync::local};

/// What names a listed member by `link` alone, as a store binding does.
fn named(link: &str) -> PimdirRemoteMeta {
    PimdirRemoteMeta {
        link_id: PimdirLinkId(link.into()),
        summary: None,
        sort_key: PimdirSortKey::default(),
        body: None,
    }
}

/// A store a newer neverest wrote is good: the refusal asks for an
/// upgrade and never for the reset that would drop it.
#[test]
fn a_store_a_newer_neverest_wrote_asks_for_an_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    drop(PimdirStore::open(dir.path()).unwrap());
    rusqlite::Connection::open(dir.path().join("pimdir.db"))
        .unwrap()
        .execute(
            "UPDATE store_meta SET format_version = 'storage-draft-99'",
            [],
        )
        .unwrap();

    let Err(err) = open_store(dir.path(), "imap", "imap") else {
        panic!("a newer store is refused");
    };

    let err = format!("{err:#}");
    assert!(err.contains("newer neverest"), "{err}");
    assert!(!err.contains("--reset"), "{err}");
}

/// A refused store with no version or an earlier draft is an earlier
/// neverest's; a later draft or a release is a newer one's.
///
/// Every draft before storage-draft-08 wrote no version.
#[test]
fn a_store_version_tells_an_earlier_neverest_from_a_newer_one() {
    let refused = |found: Option<&str>| PimdirError::Version {
        found: found.map(String::from),
    };

    assert!(written_earlier(&refused(None)));
    assert!(written_earlier(&refused(Some("storage-draft-07"))));
    assert!(written_earlier(&refused(Some("storage-draft-08"))));
    assert!(written_earlier(&refused(Some("storage-draft-09"))));
    assert!(written_earlier(&refused(Some("storage-draft-10"))));
    assert!(written_earlier(&refused(Some("storage-draft-11"))));
    assert!(written_earlier(&refused(Some("storage-draft-12"))));
    assert!(written_earlier(&refused(Some("storage-draft-13"))));
    assert!(!written_earlier(&refused(Some("storage-draft-15"))));
    assert!(!written_earlier(&refused(Some("storage-draft-x"))));
    assert!(!written_earlier(&refused(Some("storage-v1"))));
    assert!(!written_earlier(&PimdirError::Busy));
}

/// The marker survives the context a caller adds on top of the open,
/// and a failure that is not an open carries none.
#[test]
fn an_open_failure_reads_as_unreached_through_later_context() {
    let refused = anyhow!("connect 127.0.0.1:1: Connection refused")
        .context(Unreached::new("imap", "Open connection"))
        .context("Sync source imap");
    let unreached = refused.downcast_ref::<Unreached>().expect("marked");
    assert_eq!(unreached.endpoint, "imap");
    assert_eq!(
        format!("{refused:#}"),
        "Sync source imap: Open connection: connect 127.0.0.1:1: Connection refused"
    );

    let unreadable = anyhow!("NO Mailbox does not exist").context("Select INBOX");
    assert!(unreadable.downcast_ref::<Unreached>().is_none());
}
use crate::{cli::exit::Exit, sync::source_id};

fn listed(roles: &[(&str, Option<&str>)]) -> Vec<Collection> {
    roles
        .iter()
        .map(|(id, role)| Collection {
            id: id.to_string(),
            name: id.to_string(),
            total: None,
            unread: None,
            role: role.map(String::from),
            takes_tasks: None,
        })
        .collect()
}

/// A listing carrying its roles asks nothing more.
///
/// A lookup the listing needs is asked once per collection set, reused
/// while the set holds, asked again when it moves.
#[test]
fn roles_a_listing_lacks_are_looked_up_once_per_collection_set() {
    let mut state = AccountState::default();
    let carried = listed(&[
        ("INBOX", Some("inbox")),
        ("Sent", Some("sent")),
        ("Work", None),
    ]);
    let roles = stated_roles(|| Ok(None), "imap", &carried, &mut state).unwrap();
    assert_eq!(roles.len(), 2);
    assert_eq!(roles["Sent"], "sent");
    assert!(state.looked_up_roles.is_empty(), "nothing to keep");

    let folders = listed(&[("AAA", None), ("BBB", None)]);
    let found = || Ok(Some(BTreeMap::from([("BBB".into(), "sent".into())])));
    let roles = stated_roles(found, "graph", &folders, &mut state).unwrap();
    assert_eq!(roles["BBB"], "sent");

    let roles = stated_roles(
        || panic!("the same set is not looked up again"),
        "graph",
        &folders,
        &mut state,
    )
    .unwrap();
    assert_eq!(roles["BBB"], "sent", "the kept answer");

    let moved = listed(&[("AAA", None), ("BBB", None), ("CCC", None)]);
    let found = || Ok(Some(BTreeMap::from([("CCC".into(), "sent".into())])));
    let roles = stated_roles(found, "graph", &moved, &mut state).unwrap();
    assert_eq!(roles.get("BBB"), None, "the server moved its role");
    assert_eq!(roles["CCC"], "sent");

    let failed = || Err(anyhow!("offline"));
    let more = listed(&[("DDD", None)]);
    assert!(
        stated_roles(failed, "graph", &more, &mut state).is_none(),
        "a failed lookup clears nothing"
    );
}

/// Every account naming no root shares one store, `neverest/` under the
/// platform base, which `store.root` overrides for its account.
///
/// One shape everywhere keeps a store addressable by the same relative
/// path whichever platform wrote it.
#[test]
fn the_default_store_root_is_shared_under_a_platform_base() {
    let dir = default_root().unwrap();

    assert!(dir.ends_with("neverest"), "{}", dir.display());
    assert!(dir.is_absolute(), "{}", dir.display());

    let work: AccountConfig = toml::from_str("imap.server = \"w.org\"").unwrap();
    let home: AccountConfig = toml::from_str("imap.server = \"h.org\"").unwrap();
    assert_eq!(store_dir(&work).unwrap(), dir);
    assert_eq!(store_dir(&home).unwrap(), dir);

    let mut config = AccountConfig::default();
    config.store.root = Some(PathBuf::from("/tmp/elsewhere"));

    assert_eq!(store_dir(&config).unwrap(), PathBuf::from("/tmp/elsewhere"),);
}

/// A store with one body and the files a dry run writes to.
fn stub_store(dir: &Path) {
    create_store(dir).unwrap();
    let body = dir.join(BLOBS_DIR).join("ab").join("cd");
    fs::create_dir_all(&body).unwrap();
    fs::write(body.join("abcd"), b"body").unwrap();
}

/// The rule the replica rests on.
///
/// A body is shared, so a mail account's blob tree is not read and written
/// whole, and everything a dry run writes to is its own, so those writes
/// cannot reach the real store.
#[cfg(unix)]
#[test]
fn a_dry_run_replica_shares_the_bodies_and_copies_the_rest() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    stub_store(&store);

    let replica = DryRunReplica::new(&store).unwrap();

    let body = |dir: &Path| dir.join(BLOBS_DIR).join("ab").join("cd").join("abcd");
    assert_eq!(
        fs::metadata(body(&store)).unwrap().ino(),
        fs::metadata(body(&replica.dir)).unwrap().ino(),
    );
    assert_ne!(
        fs::metadata(store.join("pimdir.db")).unwrap().ino(),
        fs::metadata(replica.dir.join("pimdir.db")).unwrap().ino(),
    );
    PimdirStore::open(&replica.dir).expect("the replica opens");
}

/// A dry run's replica of a store opens as one.
///
/// A store is its user's alone (pimdir STORAGE §18), so a replica made
/// under the umask was refused as exposed and no dry run ran.
#[test]
fn a_dry_run_replica_opens_as_a_store() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    drop(PimdirStore::open(&store).unwrap());

    let replica = DryRunReplica::new(&store).unwrap();

    PimdirStore::open(&replica.dir).expect("the replica opens");
}

/// A dry run's replica carries the store's local/, neverest's own state
/// (pimdir STORAGE §3), so the account reads initialized there.
///
/// The replica still opens: io-pimdir asks a local/ its user's alone
/// before its owner opens it.
#[test]
fn a_dry_run_replica_carries_the_owners_state() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    create_store(&store).unwrap();
    let mode: AccountConfig = toml::from_str("imap.server = \"imap://127.0.0.1:1\"").unwrap();
    StoreState::stamp(&store, "a", &mode.mode().unwrap()).unwrap();

    let replica = DryRunReplica::new(&store).unwrap();

    assert!(local::path(&replica.dir).join("neverest.json").is_file());
    assert!(StoreState::load(&replica.dir).unwrap().initialized("a"));
    PimdirStore::open(&replica.dir).expect("the replica opens");
}

/// Writing to the replica's index leaves the real one alone.
///
/// That is what a dry run means, and what a shared index would break.
#[test]
fn writing_to_a_dry_run_replica_leaves_the_store_alone() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    stub_store(&store);

    let before = fs::read(store.join("pimdir.db")).unwrap();

    let replica = DryRunReplica::new(&store).unwrap();
    fs::write(replica.dir.join("pimdir.db"), b"advanced").unwrap();

    assert_eq!(fs::read(store.join("pimdir.db")).unwrap(), before);
}

/// A replica waits out a collector and copies no lock file.
///
/// It holds the staging lock shared while it takes the database and links the
/// bodies (pimdir STORAGE §8), so no collector unlinks a body it names, and a
/// lock file holds no state.
#[test]
fn a_dry_run_replica_waits_out_a_collector() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    stub_store(&store);
    let collector = fs::File::create(store.join("objects.lock")).unwrap();
    collector.lock().unwrap();

    let (sent, taken) = std::sync::mpsc::channel();
    let real = store.clone();
    let cloning = std::thread::spawn(move || {
        let replica = DryRunReplica::new(&real).unwrap();
        sent.send(()).unwrap();
        replica
    });

    let waited = taken.recv_timeout(std::time::Duration::from_millis(300));
    assert!(waited.is_err(), "the replica was taken under a collector");
    collector.unlock().unwrap();
    let replica = cloning.join().unwrap();

    assert!(!replica.dir.join("objects.lock").exists());
    PimdirStore::open(&replica.dir).expect("the replica opens");
}

/// The replica goes however the run ends, an early return included.
///
/// Which is why it is a guard rather than a line on the way out.
#[test]
fn a_dry_run_replica_is_removed_when_the_run_ends() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    stub_store(&store);

    let dir = {
        let replica = DryRunReplica::new(&store).unwrap();
        replica.dir.clone()
    };

    assert!(!dir.exists());
    assert!(store.join("pimdir.db").exists());
}

/// A release build aborts on a panic and runs no destructor.
///
/// The next run is what clears what the aborted one left behind.
#[test]
fn a_replica_an_earlier_run_left_behind_is_cleared() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("account");
    stub_store(&store);

    let stale = root.path().join(".account-dry-424242");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("pimdir.db"), b"leftover").unwrap();

    let _replica = DryRunReplica::new(&store).unwrap();

    assert!(!stale.exists());
}

#[test]
fn a_side_pair_must_agree_on_its_kind() {
    let mail = "message/rfc822";

    assert_eq!(
        pair_kind(("left", mail), ("right", mail)).unwrap(),
        Kind::Mail
    );

    let err = pair_kind(("left", mail), ("right", "text/vcard"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("text/vcard"), "{err}");
    assert!(err.contains("right"), "{err}");

    let err = pair_kind(("left", ""), ("right", mail))
        .unwrap_err()
        .to_string();
    assert!(err.contains("left"), "{err}");
}

/// An action is applied as the source syncing its collection.
///
/// On an account syncing three kinds, `caldav` sorts first and reaches
/// every mail action before `imap` does; the store now hands the action
/// to `imap` under `caldav`'s handle.
#[test]
fn a_drain_applies_an_action_as_the_source_of_its_collection() {
    let dir = tempfile::tempdir().unwrap();
    let mut imap = PimdirStore::open(dir.path()).unwrap().for_source("imap");
    imap.ensure_collection("imap/INBOX", "message/rfc822")
        .unwrap();

    let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
    let queue = |producer: &mut PimdirProducer, link: &str| {
        let id = link.trim_start_matches("mid:");
        let body = format!("Message-ID: <{id}>\r\nSubject: hi\r\n\r\nhello\r\n");
        producer
            .enqueue_with_body("imap/INBOX", body.as_bytes(), |hash| PimdirAction::Add {
                link_id: Some(PimdirLinkId(link.into())),
                flags: PimdirFlags::default(),
                object: Some(hash),
                supersedes: None,
            })
            .unwrap();
    };

    queue(&mut producer, "mid:first@x");
    let mut report = SyncOutput::default();
    drain_queues(&mut imap, &mut report);
    assert_eq!(report.drained.len(), 1);
    assert_eq!(report.drained[0].collection, "imap/INBOX");
    assert_eq!(report.drained[0].applied, 1);

    queue(&mut producer, "mid:second@x");
    let mut caldav = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
    let mut report = SyncOutput::default();
    drain_queues(&mut caldav, &mut report);
    assert_eq!(report.drained.len(), 1);
    assert_eq!(report.drained[0].collection, "imap/INBOX");
    assert_eq!(report.drained[0].applied, 1);
    assert_eq!(
        caldav.collection_sources("imap/INBOX").unwrap(),
        vec!["imap".to_string()],
        "the create is imap's to push"
    );
}

/// A parked row belongs to the store and is reported once, by its
/// collection's account.
///
/// Read where each drain runs, an account syncing three kinds showed the
/// same warning three times.
#[test]
fn a_parked_action_is_reported_once_however_many_drains_ran() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("a")
        .for_source("imap");
    store
        .ensure_collection("imap/INBOX", "message/rfc822")
        .unwrap();

    let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
    producer
        .enqueue(
            "imap/INBOX",
            &PimdirAction::SetFlags {
                seq: 6951,
                flags: PimdirFlags::from_iter(["\\Seen"]),
                base: None,
            },
            None,
        )
        .unwrap();

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    drain_queues(&mut store, &mut report);
    report_parked(&store, "a", &mut report);

    assert_eq!(report.parked.len(), 1);
    assert_eq!(report.parked[0].producer, "himalaya");

    let mut other = SyncOutput::default();
    report_parked(&store, "b", &mut other);
    assert!(other.parked.is_empty());
}

/// The pre-sync drain applies queued actions and reports parked ones.
///
/// A drained `Add` reads `Created`, a create the next sync owes the
/// source: read as `Dirty`, it stranded every queued create.
#[test]
fn the_pre_sync_drain_applies_queued_actions_and_reports_parked_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("a")
        .for_source("left");
    store
        .ensure_collection("left/INBOX", "message/rfc822")
        .unwrap();

    let object = store
        .blob_stage()
        .stage_body(b"Subject: queued\r\n\r\nhello")
        .unwrap();
    let hash = object.hash.clone();

    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    producer
        .enqueue(
            "left/INBOX",
            &PimdirAction::Add {
                link_id: Some(PimdirLinkId("mid:q1@x".into())),
                flags: PimdirFlags::from_iter(["\\Seen"]),
                object: Some(hash),
                supersedes: None,
            },
            Some(&object),
        )
        .unwrap();
    let remove = PimdirAction::Remove {
        seq: 424242,
        if_match: None,
        resolves: None,
    };
    producer.enqueue("left/INBOX", &remove, None).unwrap();
    producer
        .enqueue(
            "left/INBOX",
            &PimdirAction::SetFlags {
                seq: 424243,
                flags: PimdirFlags::from_iter(["\\Seen"]),
                base: None,
            },
            None,
        )
        .unwrap();

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    report_parked(&store, "a", &mut report);

    assert_eq!(report.drained.len(), 1);
    assert_eq!(report.drained[0].collection, "left/INBOX");
    assert_eq!(report.drained[0].applied, 2);
    assert_eq!(report.parked.len(), 1);
    assert!(report.parked[0].error.contains("unknown seq"));

    let placements = load_side(&store, "left/INBOX").unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].status, PimdirStatus::Created);
    assert!(placements[0].base.is_none());
    assert_eq!(
        placements[0].link_id.as_ref().map(|l| l.0.as_str()),
        Some("mid:q1@x")
    );

    let mut second = SyncOutput::default();
    drain_queues(&mut store, &mut second);
    report_parked(&store, "a", &mut second);
    assert!(second.drained.is_empty());
    assert_eq!(second.parked.len(), 1);
}

/// Drops `table` from the store at `dir`, as a damaged database reads.
fn drop_table(dir: &Path, table: &str) {
    let conn = rusqlite::Connection::open(dir.join("pimdir.db")).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA foreign_keys = OFF; DROP TABLE receipts; DROP TABLE {table};"
    ))
    .unwrap();
}

/// A drain whose store fails is reported, and the run is incomplete.
#[test]
fn a_drain_that_fails_is_reported_and_the_run_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("a")
        .for_source("imap");
    store
        .ensure_collection("imap/INBOX", "message/rfc822")
        .unwrap();
    drop_table(dir.path(), "queue");

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    report_parked(&store, "a", &mut report);

    let steps: Vec<FailedStep> = report.failures.iter().map(|f| f.step).collect();
    assert_eq!(steps, [FailedStep::Queue, FailedStep::Queue]);
    assert_eq!(Exit::from(&report), Exit::Incomplete);
    assert!(report.to_string().contains("Errors (2)"), "{report}");
}

/// Conflicts that cannot be counted leave the run incomplete, never in
/// sync.
#[test]
fn a_conflict_count_that_fails_leaves_the_run_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let store = PimdirStore::open(dir.path()).unwrap();
    drop_table(dir.path(), "bindings");

    let mut report = SyncOutput::default();
    count_conflicts(&store, "a", &mut report);

    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].step, FailedStep::Conflicts);
    assert_eq!(Exit::from(&report), Exit::Incomplete);
}

/// The stored checkpoint's UIDVALIDITY, as the IMAP adapter encodes it.
#[cfg(feature = "imap")]
fn stored_checkpoint_uid_validity(store: &PimdirSourceStore, collection: &str) -> Option<u32> {
    let loaded = store
        .load(
            &PimdirCollectionId(collection.to_string()),
            &PimdirLoadSelection::All,
        )
        .unwrap();
    loaded
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| crate::imap::backend::checkpoint_uid_validity(&checkpoint.0))
}

/// A scripted remote for the rekey pump: a fixed spine, rejecting pushes.
#[cfg(feature = "imap")]
struct ScriptedRemote {
    /// The new handle space: `(handle, link id)` pairs.
    spine: Vec<(String, String)>,
}

#[cfg(feature = "imap")]
impl PimdirRemote for ScriptedRemote {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let items = self
            .spine
            .iter()
            .map(|(handle, link)| PimdirRemoteItem {
                handle: PimdirHandle(handle.clone()),
                flags: PimdirFlags::default(),
                revision: None,
                meta: named(link),
            })
            .collect();
        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
            items,
            Some(PimdirCheckpoint(crate::imap::backend::encode_checkpoint(
                2, 1,
            ))),
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        Ok(handles
            .into_iter()
            .filter_map(|handle| {
                let link = self
                    .spine
                    .iter()
                    .find(|(h, _)| *h == handle.0)
                    .map(|(_, link)| link.clone())?;
                Some(PimdirFetchedItem {
                    handle,
                    link_id: PimdirLinkId(link),
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    body: None,
                    revision: None,
                })
            })
            .collect())
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        _changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        anyhow::bail!("scripted remote rejects pushes")
    }
}

#[test]
#[cfg(feature = "imap")]
fn a_rekey_carries_state_by_link_id_and_bumps_the_generation_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    store.ensure_collection("INBOX", "message/rfc822").unwrap();

    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: io_pimdir::hash::hash(b"abc"),
                    size: 3,
                },
                body: Some(b"abc".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("INBOX".into()),
                handle: PimdirHandle("1".into()),
                link_id: Some(PimdirLinkId("mid:a@x".into())),
                object: Some(io_pimdir::hash::hash(b"abc")),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: Some(io_pimdir::hash::hash(b"abc")),
                }),
                origin: None,
            }),
            PimdirWriteOp::SetCheckpoint {
                collection: PimdirCollectionId("INBOX".into()),
                checkpoint: PimdirCheckpoint(crate::imap::backend::encode_checkpoint(1, 1)),
            },
        ])
        .unwrap();
    assert_eq!(store.generation("INBOX").unwrap(), Some(1));

    let mut remote = ScriptedRemote {
        spine: vec![(String::from("7"), String::from("mid:a@x"))],
    };
    let rekey = run_verb(&mut store, &mut remote, PimdirRekey::new("INBOX")).unwrap();

    assert_eq!(rekey.rekeyed, 1);
    assert_eq!(rekey.pulled, 0);
    assert_eq!(store.generation("INBOX").unwrap(), Some(2));

    let placements = load_side(&store, "INBOX").unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].handle.0, "7");
    assert_eq!(placements[0].object, Some(io_pimdir::hash::hash(b"abc")));
    assert_eq!(placements[0].status, PimdirStatus::Clean);

    assert_eq!(stored_checkpoint_uid_validity(&store, "INBOX"), Some(2));
}

/// A sync meeting a renumbered mailbox rekeys it before listing it.
///
/// The message moves to its new UID with its staged flag, nothing is
/// dropped as deleted, and the flag is pushed to the new UID.
#[test]
#[cfg(feature = "imap")]
fn a_renumbered_mailbox_is_rekeyed_before_it_is_listed() {
    use std::sync::{Arc, atomic::AtomicU64};

    use pimalaya_stream::tls::Tls;

    use crate::{
        account::{ImapAccount, SourceAccountBackend},
        imap::fake::{FakeImap, FakeMailbox, FakeMessage},
        throttle::Throttle,
    };

    const INBOX: &str = "imap/INBOX";

    let server = FakeImap::start(
        FakeMailbox {
            uid_validity: 2,
            highest_mod_seq: 9,
            messages: vec![FakeMessage {
                uid: 7,
                flags: Vec::new(),
                message_id: String::from("a@x"),
            }],
            states_exists: true,
        },
        true,
    );

    let dir = tempfile::tempdir().unwrap();
    let mut store = open_store(dir.path(), "imap", "imap").unwrap();
    store.ensure_collection(INBOX, "message/rfc822").unwrap();
    store
        .write(vec![
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId(INBOX.into()),
                handle: PimdirHandle("1".into()),
                link_id: Some(PimdirLinkId("a@x".into())),
                object: None,
                level: PimdirLevel::Meta,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::from_iter(["\\Flagged"]),
                status: PimdirStatus::Dirty,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: None,
                }),
                origin: None,
            }),
            PimdirWriteOp::SetCheckpoint {
                collection: PimdirCollectionId(INBOX.into()),
                checkpoint: PimdirCheckpoint(crate::imap::backend::encode_checkpoint(1, 5)),
            },
        ])
        .unwrap();

    let config: AccountConfig = toml::from_str("imap.server = \"imap://127.0.0.1:1\"").unwrap();
    let perms = config.sources().unwrap()["imap"].permissions();
    let account = SourceAccount {
        backend: SourceAccountBackend::Imap(ImapAccount {
            server: server.url.clone(),
            tls: Tls::default(),
            starttls: false,
            sasl: None,
            proxy: pimalaya_stream::proxy::Proxy::None,
            sasl_ir: None,
            auto_id: None,
        }),
        #[cfg(feature = "smtp")]
        smtp: None,
        throttle: Arc::new(Throttle::new("imap")),
        downloaded: Arc::new(AtomicU64::new(0)),
    };
    let mut ctx = SourceCtx {
        name: String::from("imap"),
        namespace: String::from("imap"),
        pool: Pool::open(account, 1).unwrap(),
        perms,
        authority: Authority::Shared,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: PimdirScope::unbounded(),
        dry_run: false,
        accept_absences: false,
        capped: false,
        noted: Noted::default(),
    };

    let blobs = store.blob_stage();
    sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");

    let placements = load_side(&store, INBOX).unwrap();
    let held: Vec<_> = placements
        .iter()
        .map(|placement| {
            (
                placement.handle.0.as_str(),
                placement.flags.contains("\\Flagged"),
            )
        })
        .collect();
    assert_eq!(held, [("7", true)]);
    assert_eq!(store.generation(INBOX).unwrap(), Some(2));
    assert!(
        server
            .sent("UID STORE")
            .iter()
            .any(|line| line.starts_with("UID STORE 7 ")),
        "the staged flag is pushed to the new UID: {:?}",
        server.commands.lock().unwrap()
    );
}

/// A staged copy the target holds out of scope is bound to it, not filed.
#[test]
#[cfg(feature = "imap")]
fn a_copy_the_target_holds_out_of_scope_is_bound_not_filed_again() {
    use crate::imap::fake::{FakeImap, FakeMailbox, FakeMessage};

    const INBOX: &str = "imap/INBOX";

    let server = FakeImap::start(
        FakeMailbox {
            uid_validity: 1,
            highest_mod_seq: 9,
            messages: vec![FakeMessage {
                uid: 7,
                flags: vec![String::from("\\Seen")],
                message_id: String::from("a@x"),
            }],
            states_exists: true,
        },
        true,
    );
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, None);
    ctx.scope = PimdirScope::since("2026-11-01T00:00:00Z");
    store
        .ensure_collection("imap/Archive", "message/rfc822")
        .unwrap();
    let source = PimdirPlacement {
        collection: PimdirCollectionId("imap/Archive".into()),
        handle: PimdirHandle("3".into()),
        link_id: Some(PimdirLinkId("a@x".into())),
        object: None,
        level: PimdirLevel::Meta,
        summary: None,
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::from_iter(["\\Seen"]),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        base: Some(PimdirBase {
            flags: PimdirFlags::from_iter(["\\Seen"]),
            revision: None,
            object: None,
        }),
        origin: None,
    };
    let staged = PimdirPlacement {
        collection: PimdirCollectionId(INBOX.into()),
        handle: PimdirLinkId("a@x".into()).provisional(),
        status: PimdirStatus::Created,
        base: None,
        origin: Some(PimdirOrigin {
            collection: source.collection.clone(),
            handle: source.handle.clone(),
        }),
        ..source.clone()
    };
    store
        .write(vec![
            PimdirWriteOp::UpsertPlacement(source),
            PimdirWriteOp::UpsertPlacement(staged),
        ])
        .unwrap();

    let blobs = store.blob_stage();
    sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");

    let placements = load_side(&store, INBOX).unwrap();
    let copy = placements
        .iter()
        .find(|placement| placement.link_id == Some(PimdirLinkId("a@x".into())))
        .expect("the copy");
    assert_eq!(copy.handle.0, "7", "bound to the message found");
    assert_eq!(copy.status, PimdirStatus::Clean);
    assert!(server.sent("APPEND").is_empty(), "nothing filed again");
}

/// The outcome of pushing `kind` to a fake inbox holding `a@x` under UID 7,
/// announcing UIDPLUS when `uidplus`.
#[cfg(feature = "imap")]
fn pushed_to_imap(
    uidplus: bool,
    kind: impl FnOnce(&PimdirBlobStage) -> PimdirChangeKind,
) -> (crate::imap::fake::FakeImap, PimdirPushResult) {
    use crate::imap::fake::{FakeImap, FakeMailbox, FakeMessage};

    let server = FakeImap::start(
        FakeMailbox {
            uid_validity: 1,
            highest_mod_seq: 9,
            messages: vec![FakeMessage {
                uid: 7,
                flags: Vec::new(),
                message_id: String::from("a@x"),
            }],
            states_exists: true,
        },
        true,
    );
    if !uidplus {
        server.without_uidplus();
        server.keep_appends();
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, store) = bound_imap_inbox(dir.path(), &server, true, None);
    let blobs = store.blob_stage();
    let collection = PimdirCollectionId("imap/INBOX".into());
    let change = PimdirChange::new(&collection, kind(&blobs));

    let mut remote = PimRemote::new(&mut ctx.pool, blobs, "imap");
    let mut results = remote.push(&collection, vec![change]).unwrap();
    (server, results.remove(0))
}

/// A create its server took without naming the message is delivered.
///
/// The run reports it waiting for the listing that names it and the next
/// one appends nothing, where a rejected append was sent again by every
/// run (pimdir SYNC §4, §5).
#[test]
#[cfg(feature = "imap")]
fn a_create_appended_without_a_uid_waits_and_is_not_appended_again() {
    use crate::imap::fake::{FakeImap, FakeMailbox};

    const INBOX: &str = "imap/INBOX";
    const BODY: &str = "From: a@x\r\nSubject: undated\r\n\r\nbody\r\n";

    let server = FakeImap::start(
        FakeMailbox {
            uid_validity: 1,
            highest_mod_seq: 9,
            messages: Vec::new(),
            states_exists: true,
        },
        true,
    );
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, None);
    let blobs = store.blob_stage();
    let object = blobs.stage_body(BODY.as_bytes()).unwrap();
    let key = PimdirLinkId("alt:undated".into());
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: object.clone(),
                body: None,
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId(INBOX.into()),
                handle: key.provisional(),
                link_id: Some(key),
                object: Some(object.hash),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Created,
                conflict_revision: None,
                conflict_object: None,
                base: None,
                origin: None,
            }),
        ])
        .unwrap();

    sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");
    assert_eq!(server.sent("APPEND").len(), 1, "appended");

    sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");
    assert_eq!(server.sent("APPEND").len(), 1, "and never again");

    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    assert!(report.rejected.is_empty() && ctx.rejected.is_empty());
    assert_eq!(report.waiting.len(), 1, "{:?}", report.waiting);
    let waiting = &report.waiting[0];
    assert_eq!(waiting.side, "imap");
    assert_eq!(waiting.collection, "INBOX");
    assert_eq!(waiting.id, "alt:undated", "the key, no control character");
    assert_eq!(waiting.reason, WaitReason::Delivered);
    assert_eq!(
        serde_json::to_value(&report).unwrap()["waiting"],
        serde_json::json!([{
            "side": "imap",
            "collection": "INBOX",
            "id": "alt:undated",
            "reason": "delivered",
        }])
    );
    let text = report.to_string();
    assert!(text.contains("Waiting (1):"), "{text}");
    assert!(!text.contains("Warnings"), "waiting is no warning: {text}");
}

/// A create refused as it stands (IMAP `NO [CANNOT]`) is held, not
/// rejected: the next run appends nothing and reports it again.
///
/// Pimdir SYNC §4, §5, vectors 162, 163. A `NO` naming a quota is
/// rejected and appended again, the account's state outliving the change.
#[test]
#[cfg(feature = "imap")]
fn a_create_refused_as_it_stands_is_held_and_reported() {
    use crate::imap::fake::{FakeImap, FakeMailbox};

    const INBOX: &str = "imap/INBOX";
    const BODY: &str = "Message-ID: <held@x>\r\nSubject: held\r\n\r\nbody\r\n";

    for (completion, held) in [
        ("NO [CANNOT] Cannot store that message", true),
        ("NO [OVERQUOTA] Mailbox full", false),
    ] {
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 1,
                highest_mod_seq: 9,
                messages: Vec::new(),
                states_exists: true,
            },
            true,
        );
        let dir = tempfile::tempdir().unwrap();
        let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, None);
        let blobs = store.blob_stage();
        let object = blobs.stage_body(BODY.as_bytes()).unwrap();
        let key = PimdirLinkId("held@x".into());
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: object.clone(),
                    body: None,
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId(INBOX.into()),
                    handle: key.provisional(),
                    link_id: Some(key),
                    object: Some(object.hash),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Created,
                    conflict_revision: None,
                    conflict_object: None,
                    base: None,
                    origin: None,
                }),
            ])
            .unwrap();

        server.refuse_next_write(completion);
        sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");
        assert_eq!(server.sent("APPEND").len(), 1, "{completion}");

        server.refuse_next_write(completion);
        let mut report = SyncOutput::default();
        report.item_patch.push(PatchEntry::new(
            ItemHunk::Add {
                side: String::from("imap"),
                collection: String::from("INBOX"),
                id: String::from("held@x"),
                flags: BTreeSet::new(),
                content_key: 0,
            },
            None,
        ));
        sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");
        itemize_noted(&mut ctx, &mut report);
        itemize_rejected(&ctx.name, mem::take(&mut ctx.rejected), &mut report);

        let standing = PimdirReader::open(dir.path())
            .unwrap()
            .refusals(INBOX)
            .unwrap();
        assert!(
            report.item_patch.is_empty(),
            "{completion}: the hunk taken back"
        );
        assert_eq!(Exit::from(&report), Exit::Conflicted, "{completion}");
        if !held {
            assert_eq!(server.sent("APPEND").len(), 2, "appended again");
            assert!(standing.is_empty(), "{standing:?}");
            assert!(report.refusals.is_empty(), "{:?}", report.refusals);
            assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
            continue;
        }

        assert_eq!(server.sent("APPEND").len(), 1, "held, never appended again");
        assert_eq!(standing.len(), 1, "{standing:?}");
        assert_eq!(standing[0].source, "imap");
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        let json = serde_json::to_value(&report).unwrap();
        let refusals = json["refusals"].as_array().unwrap();
        assert_eq!(refusals.len(), 1, "{json}");
        assert_eq!(refusals[0]["side"], "imap");
        assert_eq!(refusals[0]["collection"], "INBOX");
        assert_eq!(refusals[0]["id"], "held@x", "the key, no control character");
        let reason = refusals[0]["reason"].as_str().unwrap();
        assert!(reason.contains("Cannot store that message"), "{reason}");
        let text = report.to_string();
        assert!(text.contains("Warnings (1):"), "{text}");
        assert!(
            text.contains("imap refused the change of held@x in INBOX"),
            "{text}"
        );
    }
}

/// A member named apart from a create of its side holding its identity
/// is warned once, whatever the passes that met it (pimdir SYNC §5, §6).
#[test]
#[cfg(feature = "imap")]
fn a_member_named_apart_from_its_create_is_reported_once() {
    let server = one_message(1, 7);
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, _store) = bound_imap_inbox(dir.path(), &server, true, None);
    let apart = PimdirSyncEvent::Disagreed {
        handle: PimdirHandle("7".into()),
        link_id: PimdirLinkId("m1@x".into()),
    };

    let events = [apart];
    ctx.noted.note_events("imap/INBOX", &events);
    ctx.noted.note_events("imap/INBOX", &events);
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);

    assert_eq!(
        serde_json::to_value(&report).unwrap()["disagreed"],
        serde_json::json!([{
            "side": "imap",
            "collection": "INBOX",
            "id": "7",
            "create": "m1@x",
        }])
    );
    let text = report.to_string();
    assert!(text.contains("1 warnings"), "{text}");
    assert!(text.contains("kept apart"), "{text}");
}

/// A capped source states it on every page of a round (pimdir SYNC §4,
/// §5).
///
/// The ten bound messages its listing leaves out, under the half that
/// parks, are kept and counted, where an uncapped source drops them.
#[test]
#[cfg(feature = "imap")]
fn a_capped_source_keeps_what_its_listing_leaves_out() {
    use crate::imap::fake::{FakeImap, FakeMailbox, FakeMessage};

    const INBOX: &str = "imap/INBOX";

    let newest = || {
        FakeImap::start(
            FakeMailbox {
                uid_validity: 1,
                highest_mod_seq: 9,
                messages: (11..=25)
                    .map(|uid| FakeMessage {
                        uid,
                        flags: Vec::new(),
                        message_id: format!("m{uid}@x"),
                    })
                    .collect(),
                states_exists: true,
            },
            false,
        )
    };
    let held = |store: &PimdirSourceStore| load_side(store, INBOX).unwrap().len();

    let server = newest();
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, false, None);
    let blobs = store.blob_stage();
    sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");
    assert_eq!(held(&store), 15, "a listing not capped proves its absences");
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    assert!(report.unlisted.is_empty(), "{:?}", report.unlisted);

    let server = newest();
    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, false, None);
    let config: AccountConfig =
        toml::from_str("imap.server = \"imap://127.0.0.1:1\"\nimap.collection.capped = true")
            .unwrap();
    ctx.capped = config.sources().unwrap()["imap"].collection().capped;
    let blobs = store.blob_stage();
    let (synced, _) =
        sync_side_rebuilding(INBOX, &mut ctx, &mut store, &blobs, true, None).expect("the sync");

    assert_eq!(synced.unlisted, 10);
    assert_eq!(synced.parked, None, "the round closes as any other");
    assert_eq!(held(&store), 25, "a capped listing proves no absence");
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    assert_eq!(
        serde_json::to_value(&report).unwrap()["unlisted"],
        serde_json::json!([{ "side": "imap", "collection": "INBOX", "count": 10 }])
    );
    let text = report.to_string();
    assert!(text.contains("Unlisted (1):"), "{text}");
    assert!(text.contains("10 item(s) of INBOX"), "{text}");
    assert!(!text.contains("Warnings"), "unlisted is no warning: {text}");
}

/// A remove carrying a target is a relocation, and its outcome says so,
/// which is what delivers the target's create (pimdir SYNC §4).
#[test]
#[cfg(feature = "imap")]
fn a_remove_relocating_its_member_says_so() {
    let (server, result) = pushed_to_imap(true, |_| PimdirChangeKind::Remove {
        handle: PimdirHandle("7".into()),
        to: Some(PimdirCollectionId("imap/Archive".into())),
        link_id: Some(PimdirLinkId("b@x".into())),
        if_match: None,
    });

    assert_eq!(result.outcome, PimdirPushOutcome::Accepted);
    assert!(result.relocated, "the member went into the target");
    assert_eq!(server.sent("UID MOVE"), ["UID MOVE 7 Archive"]);
    assert!(server.sent("UID EXPUNGE").is_empty());
}

/// A remove whose target already holds the identity deletes the member
/// rather than filing it twice.
///
/// It states no relocation, so the target's create stays pending (pimdir
/// SYNC §4, §5).
#[test]
#[cfg(feature = "imap")]
fn a_remove_into_a_target_holding_the_identity_deletes() {
    let (server, result) = pushed_to_imap(true, |_| PimdirChangeKind::Remove {
        handle: PimdirHandle("7".into()),
        to: Some(PimdirCollectionId("imap/Archive".into())),
        link_id: Some(PimdirLinkId("a@x".into())),
        if_match: None,
    });

    assert_eq!(result.outcome, PimdirPushOutcome::Accepted);
    assert!(!result.relocated, "the target held it: nothing relocated");
    assert!(server.sent("UID MOVE").is_empty(), "filed no second time");
    assert_eq!(server.sent("UID EXPUNGE"), ["UID EXPUNGE 7"]);
}

/// A remove under a minted key never asks its target.
///
/// The target held the identity before the move, which proves no delivery
/// (pimdir SYNC §4).
#[test]
#[cfg(feature = "imap")]
fn a_remove_under_a_minted_key_relocates_unasked() {
    let (server, result) = pushed_to_imap(true, |_| PimdirChangeKind::Remove {
        handle: PimdirHandle("7".into()),
        to: Some(PimdirCollectionId("imap/Archive".into())),
        link_id: Some(PimdirLinkId("dup:a@x#7".into())),
        if_match: None,
    });

    assert!(result.relocated);
    assert!(server.sent("UID SEARCH").is_empty());
    assert_eq!(server.sent("UID MOVE"), ["UID MOVE 7 Archive"]);
}

/// An append taken without a UID, which no search finds either, is
/// accepted with no handle.
///
/// The create is delivered and the next listing lands it, where a
/// rejection had it appended again (pimdir SYNC §4).
#[test]
#[cfg(feature = "imap")]
fn an_append_answered_no_uid_is_accepted_with_no_handle() {
    const BODY: &str = "From: a@x\r\nSubject: undated\r\n\r\nbody\r\n";

    let (server, result) = pushed_to_imap(true, |blobs| PimdirChangeKind::Add {
        handle: PimdirLinkId("alt:undated".into()).provisional(),
        link_id: Some(PimdirLinkId("alt:undated".into())),
        flags: PimdirFlags::default(),
        origin: None,
        supersedes: None,
        object: Some(blobs.stage_body(BODY.as_bytes()).unwrap().hash),
    });

    assert_eq!(server.sent("APPEND").len(), 1, "appended once");
    assert_eq!(result.outcome, PimdirPushOutcome::Accepted);
    assert_eq!(result.assigned, None);
    assert!(!result.relocated);
}

/// A remove whose target holds the identity, on a server with no UIDPLUS, is
/// held rather than relocated.
///
/// The server cannot expunge the one message, so the member stays where it
/// is, neither filed a second time nor marked `\Deleted` (pimdir SYNC §4,
/// §5).
#[test]
#[cfg(feature = "imap")]
fn a_remove_into_a_target_holding_the_identity_without_uidplus_is_held() {
    let (server, result) = pushed_to_imap(false, |_| PimdirChangeKind::Remove {
        handle: PimdirHandle("7".into()),
        to: Some(PimdirCollectionId("imap/Archive".into())),
        link_id: Some(PimdirLinkId("a@x".into())),
        if_match: None,
    });

    assert_ne!(result.outcome, PimdirPushOutcome::Accepted);
    assert!(!result.relocated, "the target held it: nothing relocated");
    assert!(server.sent("UID MOVE").is_empty(), "filed no second time");
    assert!(
        server.sent("UID STORE").is_empty(),
        "nothing marked deleted"
    );
}

/// An append to a server with no UIDPLUS is accepted with the UID a search
/// on its `Message-ID` finds, never failed after it landed (pimdir SYNC §4).
#[test]
#[cfg(feature = "imap")]
fn an_append_to_a_server_without_uidplus_is_found_by_its_message_id() {
    const BODY: &str = "Message-ID: <new@x>\r\nFrom: a@x\r\nSubject: new\r\n\r\nbody\r\n";

    let (server, result) = pushed_to_imap(false, |blobs| PimdirChangeKind::Add {
        handle: PimdirLinkId("new@x".into()).provisional(),
        link_id: Some(PimdirLinkId("new@x".into())),
        flags: PimdirFlags::default(),
        origin: None,
        supersedes: None,
        object: Some(blobs.stage_body(BODY.as_bytes()).unwrap().hash),
    });

    assert_eq!(server.sent("APPEND").len(), 1, "appended once");
    assert_eq!(result.outcome, PimdirPushOutcome::Accepted);
    assert_eq!(result.assigned, Some(PimdirHandle("8".into())));
    let searched = server.sent("UID SEARCH");
    assert!(
        searched.iter().any(|line| line.contains("new@x")),
        "{searched:?}"
    );
}

/// A create holding no body names no member of its server yet.
///
/// Pending or delivered, its provisional handle is owed no body fetch,
/// which no server could answer (pimdir SYNC §2).
#[test]
#[cfg(feature = "imap")]
fn a_create_naming_no_member_is_owed_no_body_fetch() {
    const INBOX: &str = "imap/INBOX";

    let server = one_message(1, 7);
    let dir = tempfile::tempdir().unwrap();
    let (_ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, None);
    let create = |key: &str| PimdirPlacement {
        collection: PimdirCollectionId(INBOX.into()),
        handle: PimdirLinkId(key.into()).provisional(),
        link_id: Some(PimdirLinkId(key.into())),
        object: None,
        level: PimdirLevel::Meta,
        summary: None,
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::default(),
        status: PimdirStatus::Created,
        conflict_revision: None,
        conflict_object: None,
        base: None,
        origin: None,
    };
    store
        .write(vec![
            PimdirWriteOp::UpsertPlacement(create("pending@x")),
            PimdirWriteOp::UpsertPlacement(create("delivered@x")),
            PimdirWriteOp::DeliverPlacement {
                collection: PimdirCollectionId(INBOX.into()),
                handle: PimdirLinkId("delivered@x".into()).provisional(),
            },
        ])
        .unwrap();
    let delivered = load_side(&store, INBOX)
        .unwrap()
        .into_iter()
        .filter(PimdirPlacement::delivered)
        .count();
    assert_eq!(delivered, 1, "the store holds the delivered create");

    let collections = vec![String::from(INBOX)];
    let plans = hydrate_plans(&store, "imap", &collections, None, &GoneBodies::default()).unwrap();

    let provisional: Vec<&str> = plans[0]
        .1
        .iter()
        .filter(|target| target.handle.is_provisional())
        .map(|target| target.handle.as_str())
        .collect();
    assert!(provisional.is_empty(), "{provisional:?}");
    assert_eq!(plans[0].1.len(), 25, "the bound members are still owed");
}

/// An IMAP side over `server` and a store binding 25 inbox messages.
///
/// Absences are accepted when `accept`; the bindings sit under
/// `UIDVALIDITY` 1, UIDs 1 to 25, with `checkpoint` when given.
#[cfg(feature = "imap")]
fn bound_imap_inbox(
    dir: &Path,
    server: &crate::imap::fake::FakeImap,
    accept: bool,
    checkpoint: Option<Vec<u8>>,
) -> (SourceCtx, PimdirSourceStore) {
    use std::sync::{Arc, atomic::AtomicU64};

    use pimalaya_stream::tls::Tls;

    use crate::{
        account::{ImapAccount, SourceAccountBackend},
        throttle::Throttle,
    };

    let mut store = open_store(dir, "imap", "imap").unwrap();
    store
        .ensure_collection("imap/INBOX", "message/rfc822")
        .unwrap();
    let mut writes: Vec<PimdirWriteOp> = (1..=25)
        .map(|uid| {
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("imap/INBOX".into()),
                handle: PimdirHandle(uid.to_string()),
                link_id: Some(PimdirLinkId(format!("m{uid}@x"))),
                object: None,
                level: PimdirLevel::Meta,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: None,
                }),
                origin: None,
            })
        })
        .collect();
    if let Some(checkpoint) = checkpoint {
        writes.push(PimdirWriteOp::SetCheckpoint {
            collection: PimdirCollectionId("imap/INBOX".into()),
            checkpoint: PimdirCheckpoint(checkpoint),
        });
    }
    store.write(writes).unwrap();

    let config: AccountConfig = toml::from_str("imap.server = \"imap://127.0.0.1:1\"").unwrap();
    let perms = config.sources().unwrap()["imap"].permissions();
    let account = SourceAccount {
        backend: SourceAccountBackend::Imap(ImapAccount {
            server: server.url.clone(),
            tls: Tls::default(),
            starttls: false,
            sasl: None,
            proxy: pimalaya_stream::proxy::Proxy::None,
            sasl_ir: None,
            auto_id: None,
        }),
        #[cfg(feature = "smtp")]
        smtp: None,
        throttle: Arc::new(Throttle::new("imap")),
        downloaded: Arc::new(AtomicU64::new(0)),
    };
    let ctx = SourceCtx {
        name: String::from("imap"),
        namespace: String::from("imap"),
        pool: Pool::open(account, 1).unwrap(),
        perms,
        authority: Authority::Shared,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: PimdirScope::unbounded(),
        dry_run: false,
        accept_absences: accept,
        capped: false,
        noted: Noted::default(),
    };
    (ctx, store)
}

/// The fake server's one message, `m1@x` under `uid` and
/// `uid_validity`.
#[cfg(feature = "imap")]
fn one_message(uid_validity: u32, uid: u32) -> crate::imap::fake::FakeImap {
    use crate::imap::fake::{FakeImap, FakeMailbox, FakeMessage};

    FakeImap::start(
        FakeMailbox {
            uid_validity,
            highest_mod_seq: 9,
            messages: vec![FakeMessage {
                uid,
                flags: Vec::new(),
                message_id: String::from("m1@x"),
            }],
            states_exists: true,
        },
        false,
    )
}

/// The live handles of the inbox.
#[cfg(feature = "imap")]
fn live_inbox(store: &PimdirSourceStore) -> usize {
    load_side(store, "imap/INBOX")
        .unwrap()
        .iter()
        .filter(|placement| placement.status != PimdirStatus::Tombstone)
        .count()
}

/// A round listing one of 25 bound messages parks (pimdir SYNC §5).
///
/// Nothing is dropped and the report names the absence, waiting for a
/// person; `--accept-absences` drops them.
#[test]
#[cfg(feature = "imap")]
fn a_round_missing_most_of_the_inbox_parks_until_accepted() {
    let server = one_message(1, 1);

    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, false, None);
    let blobs = store.blob_stage();
    sync_side_rebuilding("imap/INBOX", &mut ctx, &mut store, &blobs, true, None).unwrap();

    assert_eq!(live_inbox(&store), 25, "nothing dropped");
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    let absences: Vec<_> = report
        .absences
        .iter()
        .map(|a| {
            (
                a.side.as_str(),
                a.collection.as_str(),
                a.absent,
                a.bound,
                a.rebuild,
            )
        })
        .collect();
    assert_eq!(absences, [("imap", "INBOX", 24, 25, false)]);
    assert_eq!(Exit::from(&report), Exit::Conflicted);

    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, None);
    let blobs = store.blob_stage();
    sync_side_rebuilding("imap/INBOX", &mut ctx, &mut store, &blobs, true, None).unwrap();

    assert_eq!(live_inbox(&store), 1, "the absences accepted");
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    assert!(report.absences.is_empty());
}

/// A renumbered mailbox listing one of 25 bound messages parks its
/// rebuild (pimdir SYNC §8).
///
/// The store keeps its old handles and checkpoint and the report names
/// the rebuild; `--accept-absences` rebuilds it.
#[test]
#[cfg(feature = "imap")]
fn a_rebuild_missing_most_of_the_inbox_parks_until_accepted() {
    let server = one_message(2, 100);
    let checkpoint = || Some(crate::imap::backend::encode_checkpoint(1, 25));

    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, false, checkpoint());
    let blobs = store.blob_stage();
    sync_side_rebuilding("imap/INBOX", &mut ctx, &mut store, &blobs, true, None).unwrap();

    assert_eq!(live_inbox(&store), 25, "nothing dropped");
    assert_eq!(store.generation("imap/INBOX").unwrap(), Some(1));
    let mut report = SyncOutput::default();
    itemize_noted(&mut ctx, &mut report);
    let absences: Vec<_> = report
        .absences
        .iter()
        .map(|a| (a.absent, a.bound, a.rebuild))
        .collect();
    assert_eq!(absences, [(24, 25, true)]);

    let dir = tempfile::tempdir().unwrap();
    let (mut ctx, mut store) = bound_imap_inbox(dir.path(), &server, true, checkpoint());
    let blobs = store.blob_stage();
    sync_side_rebuilding("imap/INBOX", &mut ctx, &mut store, &blobs, true, None).unwrap();

    assert_eq!(live_inbox(&store), 1, "rebuilt onto the new UIDs");
    assert_eq!(store.generation("imap/INBOX").unwrap(), Some(2));
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

/// The hunks as `(kind, side, collection)`, the hunk type having no
/// equality of its own.
fn hunk_list(hunks: &[CollectionHunk]) -> Vec<(&'static str, String, String)> {
    hunks
        .iter()
        .map(|hunk| match hunk {
            CollectionHunk::Create { side, collection } => {
                ("create", side.clone(), collection.clone())
            }
            CollectionHunk::Delete { side, collection } => {
                ("delete", side.clone(), collection.clone())
            }
            CollectionHunk::Scan { .. } => ("scan", String::new(), String::new()),
            CollectionHunk::Sync { .. } => ("sync", String::new(), String::new()),
        })
        .collect()
}

const GRANTED: CollectionPermissions = CollectionPermissions {
    create: true,
    delete: true,
    rename: None,
};

#[test]
fn a_collection_new_on_one_side_is_created_on_the_other() {
    let hunks = diff_collections(
        &names(&["both", "new-a"]),
        &names(&["both", "new-b"]),
        &names(&[]),
        ("a", &GRANTED),
        ("b", &GRANTED),
    );

    assert_eq!(
        hunk_list(&hunks),
        vec![
            ("create", "b".into(), "new-a".into()),
            ("create", "a".into(), "new-b".into()),
        ]
    );
}

#[test]
fn a_collection_both_held_and_one_deleted_is_deleted_on_the_other() {
    let hunks = diff_collections(
        &names(&["kept", "gone-on-b"]),
        &names(&["kept", "gone-on-a"]),
        &names(&["gone-on-a", "gone-on-b"]),
        ("a", &GRANTED),
        ("b", &GRANTED),
    );

    assert_eq!(
        hunk_list(&hunks),
        vec![
            ("delete", "a".into(), "gone-on-b".into()),
            ("delete", "b".into(), "gone-on-a".into()),
        ]
    );
}

#[test]
fn a_delete_the_remaining_side_forbids_is_not_undone_by_a_create() {
    let keep = CollectionPermissions {
        create: true,
        delete: false,
        rename: None,
    };
    let hunks = diff_collections(
        &names(&["gone-on-b"]),
        &names(&[]),
        &names(&["gone-on-b"]),
        ("a", &keep),
        ("b", &GRANTED),
    );

    assert!(hunks.is_empty(), "{hunks:?}");
}

#[test]
fn no_collection_name_is_reserved() {
    let collections: BTreeMap<String, String> = ["INBOX", "Outbox", "Sent"]
        .map(|name| (String::from(name), String::from(name)))
        .into();

    let filtered = filter_collections(&collections, &CollectionFilter::All);
    assert_eq!(filtered.len(), 3);
    assert!(filtered.contains("Outbox"));

    let filtered = filter_collections(
        &collections,
        &CollectionFilter::Include(vec![String::from("Outbox")]),
    );
    assert_eq!(filtered, BTreeSet::from([String::from("Outbox")]));
}

/// A collection is named without the namespace that addresses it.
///
/// A CalDAV listing names it by its `DAV:displayname`; a source with
/// nothing to say leaves the bare id, the IMAP mailbox or DAV segment.
#[test]
fn a_collection_is_named_without_the_namespace_that_addresses_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
    let read = || {
        PimdirReader::open(dir.path())
            .unwrap()
            .list_collections()
            .unwrap()
    };

    let dav = BTreeMap::from([(String::from("ED99C7C8"), String::from("Work"))]);
    store
        .ensure_collection("caldav/ED99C7C8", "text/calendar")
        .unwrap();
    declare_name(&store, "caldav/ED99C7C8", "ED99C7C8", &[&dav]);

    let named = &read()[0];
    assert_eq!(named.id, "caldav/ED99C7C8", "the address keeps its prefix");
    assert_eq!(named.name, "Work", "the label carries neither");

    let silent = BTreeMap::from([(String::from("Archives"), String::new())]);
    store
        .ensure_collection("caldav/Archives", "text/calendar")
        .unwrap();
    declare_name(&store, "caldav/Archives", "Archives", &[&silent, &dav]);

    let fallback = read().into_iter().find(|c| c.id.ends_with("Archives"));
    assert_eq!(fallback.unwrap().name, "Archives");
}

#[test]
fn a_submit_intent_survives_the_drain_and_is_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    store
        .ensure_collection("left/Sent", "message/rfc822")
        .unwrap();

    let object = store
        .blob_stage()
        .stage_body(b"Subject: hi\r\n\r\nhello")
        .unwrap();
    let hash = object.hash.clone();

    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    producer
        .enqueue(
            "left/Sent",
            &PimdirAction::Unknown {
                kind: submit::SUBMIT.into(),
                payload:
                    r#"{"v":1,"source":"left","from":"a@x.org","rcpts":["b@y.org"],"subject":"hi"}"#
                        .into(),
                object_hash: Some(hash.clone()),
            },
            Some(&object),
        )
        .unwrap();

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    assert!(report.drained.is_empty());
    assert!(report.parked.is_empty());
    assert!(load_side(&store, "left/Sent").unwrap().is_empty());

    let intents = submit::pending(&store).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].collection, "left/Sent");
    assert_eq!(intents[0].subject().as_deref(), Some("hi"));
    assert_eq!(intents[0].object.as_ref(), Some(&hash));

    assert!(store.drop_action(intents[0].id).unwrap());
    assert!(submit::pending(&store).unwrap().is_empty());
}

/// A send naming one of the account's targets parks: a target sends
/// nothing, an account sending through a source, so it would wait for ever.
///
/// One naming the source is left for the send channel.
#[test]
fn a_send_naming_a_target_parks() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    store
        .ensure_collection("left/Sent", "message/rfc822")
        .unwrap();
    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    for sender in ["dst", "left"] {
        let object = store
            .blob_stage()
            .stage_body(format!("Subject: {sender}\r\n\r\nhello").as_bytes())
            .unwrap();
        producer
            .enqueue(
                "left/Sent",
                &PimdirAction::Unknown {
                    kind: submit::SUBMIT.into(),
                    payload: format!(
                        r#"{{"v":1,"source":"{sender}","from":"a@x.org","rcpts":["b@y.org"],"subject":"{sender}"}}"#
                    ),
                    object_hash: Some(object.hash.clone()),
                },
                Some(&object),
            )
            .unwrap();
    }
    drop(producer);
    let config: AccountConfig = toml::from_str(
        "sources.left.imap.server = \"imap.example.org\"\n\
         targets.dst.imap.server = \"imap.example.net\"\n",
    )
    .unwrap();

    let intents = submit::pending(&store).unwrap();
    let mut report = SyncOutput::default();
    let kept = park_targeted(&config, &mut store, intents, &mut report);

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].subject().as_deref(), Some("left"));
    assert_eq!(report.submitted.len(), 1);
    assert!(report.submitted[0].parked);
    assert_eq!(report.submitted[0].subject.as_deref(), Some("dst"));
    assert_eq!(store.parked_actions().unwrap().len(), 1);
}

/// A store whose `gcal` source declares the intents on `gcal/primary`,
/// with one `calendar-reply` queued there naming `target`.
fn queued_reply(dir: &Path, target: &str) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir)
        .unwrap()
        .for_account("live")
        .for_source("gcal");
    store
        .ensure_collection("gcal/primary", "text/calendar")
        .unwrap();
    let config: AccountConfig = toml::from_str(r#"gcal.auth.token.raw = "token""#).unwrap();
    let source = config.sources().unwrap().remove("gcal").unwrap();
    let declared = capability::declaration(
        &source,
        false,
        false,
        &["gcal/primary".into()],
        &BTreeSet::new(),
    );
    store.declare("gcal", &declared).unwrap();

    PimdirProducer::open(dir, "test-frontend")
        .unwrap()
        .enqueue(
            "gcal/primary",
            &PimdirAction::Unknown {
                kind: invitation::REPLY.into(),
                payload: format!(r#"{{"v":1,"source":"gcal",{target},"partstat":"ACCEPTED"}}"#),
                object_hash: None,
            },
            None,
        )
        .unwrap();
    store
}

#[test]
fn a_calendar_intent_survives_the_drain_where_its_source_performs_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = queued_reply(dir.path(), r#""seq":1"#);

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    assert!(report.parked.is_empty());

    let intents = invitation::pending(&store).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].collection, "gcal/primary");
    assert_eq!(intents[0].source().as_deref(), Some("gcal"));
}

#[test]
fn an_intent_on_a_gone_item_parks_and_one_on_a_pending_add_waits() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = queued_reply(dir.path(), r#""seq":42"#);
    let intent = invitation::pending(&store).unwrap().remove(0);

    let failure = locate_invitation(&store, &intent).unwrap_err();
    assert!(failure.parks(), "{:#}", failure.error());

    let mut report = SyncOutput::default();
    settle_invitation(&mut store, &intent, "gcal", Err(failure), &mut report);
    assert!(invitation::pending(&store).unwrap().is_empty());
    assert_eq!(store.parked_actions().unwrap().len(), 1);
    assert!(report.intents[0].parked);
    assert_eq!(report.intents[0].seq, Some(42));

    let dir = tempfile::tempdir().unwrap();
    let store = queued_reply(dir.path(), r#""link_id":"uid:not-yet""#);
    let intent = invitation::pending(&store).unwrap().remove(0);
    assert!(!locate_invitation(&store, &intent).unwrap_err().parks());
}

/// A performed intent leaves a receipt, so its producer reads it applied
/// rather than withdrawn (pimdir STORAGE §15.4, §15.5).
#[test]
fn a_performed_intent_reads_as_applied_by_its_producer() {
    use io_pimdir::client::producer::PimdirActionStatus;

    let dir = tempfile::tempdir().unwrap();
    let mut store = queued_reply(dir.path(), r#""seq":1"#);
    let intent = invitation::pending(&store).unwrap().remove(0);

    let mut report = SyncOutput::default();
    settle_invitation(&mut store, &intent, "gcal", Ok(()), &mut report);
    assert!(invitation::pending(&store).unwrap().is_empty());
    assert!(report.intents[0].error.is_none());

    let producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    let PimdirActionStatus::Applied {
        collection, seq, ..
    } = producer.action_status(intent.id).unwrap()
    else {
        panic!("a performed intent leaves its receipt");
    };
    assert_eq!(collection.as_deref(), Some("gcal/primary"));
    assert_eq!(seq, None);
}

/// A calendar intent a killed run left in flight parks unconfirmed rather
/// than run again: the provider may have told the attendees already.
///
/// The mark is cleared, so the next intent runs as usual.
#[test]
fn a_calendar_intent_left_in_flight_parks_unconfirmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = queued_reply(dir.path(), r#""seq":1"#);
    let intent = invitation::pending(&store).unwrap().remove(0);
    let in_flight = submit::InFlight::inviting(dir.path());
    in_flight.begin(intent.id).unwrap();

    let mut report = SyncOutput::default();
    drain_invitations(&mut [], &mut store, dir.path(), &mut report);

    assert!(invitation::pending(&store).unwrap().is_empty());
    let parked = store.parked_actions().unwrap();
    assert_eq!(parked.len(), 1);
    assert!(parked[0].unconfirmed().is_some(), "{parked:?}");
    assert!(report.intents[0].parked);
    assert_eq!(in_flight.left(), None);
}

const TODO: &str = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t1\r\n\
                    DTSTAMP:20261028T090000Z\r\nSUMMARY:Call Ann\r\n\
                    END:VTODO\r\nEND:VCALENDAR\r\n";

/// A calendar of account `live` the `caldav` source syncs, holding one
/// event it binds.
fn bound_calendar(store: &mut PimdirSourceStore, collection: &str) {
    store
        .ensure_collection(collection, "text/calendar")
        .unwrap();
    store
        .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId(collection.into()),
            handle: PimdirHandle("e0.ics".into()),
            link_id: Some(PimdirLinkId(format!("{collection}-e0"))),
            object: None,
            level: PimdirLevel::Meta,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: Some(PimdirBase {
                flags: PimdirFlags::default(),
                revision: None,
                object: None,
            }),
            origin: None,
        })])
        .unwrap();
}

/// Queues the add of a new task into `collection`, as a frontend does.
fn queue_task(dir: &Path, collection: &str) -> Result<PimdirEnqueued, PimdirError> {
    let mut producer = PimdirProducer::open(dir, "test-frontend").unwrap();
    producer.enqueue_with_body(collection, TODO.as_bytes(), |hash| PimdirAction::Add {
        link_id: None,
        flags: PimdirFlags::default(),
        object: Some(hash),
        supersedes: None,
    })
}

/// A calendar whose server keeps it from tasks refuses one at the gate,
/// before it is queued and pushed (pimdir STORAGE §15.6).
///
/// The refusal comes from what the listing stated and the sidecar kept
/// for the next declaration; a later listing replaces what the earlier
/// one stated.
#[test]
fn an_events_only_calendar_refuses_a_task_before_it_is_queued() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("live")
        .for_source("caldav");
    bound_calendar(&mut store, "caldav/events");
    bound_calendar(&mut store, "caldav/work");
    bound_calendar(&mut store, "caldav/any");
    drop(store);

    let config: AccountConfig = toml::from_str(
        "caldav.server = \"https://dav.example.org/\"\n\
         caldav.auth.basic.username = \"user\"\n\
         caldav.auth.basic.password.raw = \"pw\"",
    )
    .unwrap();
    let mode = config.mode().unwrap();
    let listed = |tasks: [Option<bool>; 3]| -> Vec<Collection> {
        ["events", "work", "any"]
            .into_iter()
            .zip(tasks)
            .map(|(id, takes_tasks)| Collection {
                id: id.into(),
                name: id.into(),
                total: None,
                unread: None,
                role: None,
                takes_tasks,
            })
            .collect()
    };

    let mut state = AccountState::default();
    let stated = taskless_ids("caldav", &listed([Some(false), Some(true), None]));
    record_taskless(&mut state, "caldav", &stated);
    let mut sidecar = StoreState::default();
    sidecar.accounts.insert("live".into(), state);
    sidecar.save(dir.path()).unwrap();
    let state = StoreState::load(dir.path())
        .unwrap()
        .account("live")
        .unwrap();
    declare(dir.path(), "live", &config, &mode, &state).unwrap();

    let Err(PimdirError::Unsupported(refusal)) = queue_task(dir.path(), "caldav/events") else {
        panic!("a task into an events-only calendar is refused at the gate");
    };
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
    assert_eq!(refusal.source, "caldav");
    queue_task(dir.path(), "caldav/work").unwrap();
    queue_task(dir.path(), "caldav/any").unwrap();

    let reader = PimdirReader::open(dir.path()).unwrap();
    assert!(!reader.accepts_vtodo("caldav/events").unwrap());
    assert!(reader.accepts_vtodo("caldav/work").unwrap());
    assert!(reader.accepts_vtodo("caldav/any").unwrap());
    drop(reader);

    let mut state = state;
    let stated = taskless_ids("caldav", &listed([None, Some(true), None]));
    record_taskless(&mut state, "caldav", &stated);
    assert!(state.taskless.is_empty());
    declare(dir.path(), "live", &config, &mode, &state).unwrap();
    queue_task(dir.path(), "caldav/events").unwrap();
}

const CALDAV: &str = "caldav.server = \"https://dav.example.org/\"\n\
                      caldav.auth.basic.username = \"user\"\n\
                      caldav.auth.basic.password.raw = \"pw\"\n";

/// A calendar as a listing answers it, stating whether it takes tasks.
fn calendar(id: &str, takes_tasks: Option<bool>) -> Collection {
    Collection {
        id: id.into(),
        name: id.into(),
        total: None,
        unread: None,
        role: None,
        takes_tasks,
    }
}

/// The calendars `source` of account `live` syncs, each holding one
/// event it binds, as its first item sync leaves them.
fn synced(dir: &Path, source: &str, collections: &[&str]) {
    let mut store = PimdirStore::open(dir)
        .unwrap()
        .for_account("live")
        .for_source(source);
    for collection in collections {
        bound_calendar(&mut store, collection);
    }
}

/// The refusal of a task queued into `collection`, which must be one.
fn refused_task(dir: &Path, collection: &str) -> io_pimdir::capability::PimdirRefusal {
    match queue_task(dir, collection) {
        Err(PimdirError::Unsupported(refusal)) => refusal,
        other => panic!("a task into {collection} is refused at the gate: {other:?}"),
    }
}

/// Google Calendar and Graph keep tasks apart: their calendars refuse a
/// task at the gate from the configuration alone, no listing needed.
#[test]
fn a_google_or_graph_calendar_refuses_a_task_before_it_is_queued() {
    let sources = [
        (
            "gcal.auth.token.raw = \"token\"",
            "gcal",
            "gcal/primary",
            "Google Tasks",
        ),
        (
            "msgraph.auth.token.raw = \"token\"\nmsgraph.calendar = true",
            "msgraph-calendar",
            "msgraph-calendar/AAMk",
            "Microsoft To Do",
        ),
    ];
    for (toml, source, collection, tasks) in sources {
        let dir = tempfile::tempdir().unwrap();
        synced(dir.path(), source, &[collection]);
        let config: AccountConfig = toml::from_str(toml).unwrap();
        let mode = config.mode().unwrap();
        declare(dir.path(), "live", &config, &mode, &AccountState::default()).unwrap();

        let refusal = refused_task(dir.path(), collection);
        assert_eq!(
            refusal.capability,
            io_pimdir::capability::CALENDAR_COMPONENT_VTODO
        );
        assert_eq!(refusal.source, source);
        assert!(refusal.detail.unwrap().contains(tasks));
        let reader = PimdirReader::open(dir.path()).unwrap();
        assert!(!reader.accepts_vtodo(collection).unwrap());
    }
}

/// A Graph mail source takes an add in its Drafts folder alone.
///
/// It declares `mail.message.add` there only, so an add elsewhere is
/// refused at the gate rather than rejected by every sync after.
#[test]
fn a_graph_source_takes_an_add_in_its_drafts_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("live")
        .for_source("msgraph-mail");
    for collection in ["msgraph-mail/Drafts", "msgraph-mail/Archive"] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
        store
            .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId(collection.into()),
                handle: PimdirHandle("m0".into()),
                link_id: Some(PimdirLinkId(format!("{collection}-m0"))),
                object: None,
                level: PimdirLevel::Meta,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: None,
                }),
                origin: None,
            })])
            .unwrap();
    }
    store
        .set_collection_role("msgraph-mail/Drafts", Some("drafts"))
        .unwrap();
    let config: AccountConfig =
        toml::from_str("msgraph.auth.token.raw = \"token\"\nmsgraph.mail = true").unwrap();
    let mode = config.mode().unwrap();
    declare(dir.path(), "live", &config, &mode, &AccountState::default()).unwrap();

    let add = |collection: &str| {
        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        let message = b"Message-ID: <a@x>\r\nSubject: x\r\n\r\nbody\r\n";
        producer.enqueue_with_body(collection, message, |hash| PimdirAction::Add {
            link_id: None,
            flags: PimdirFlags::default(),
            object: Some(hash),
            supersedes: None,
        })
    };
    match add("msgraph-mail/Archive") {
        Err(PimdirError::Unsupported(refusal)) => {
            assert_eq!(refusal.capability, io_pimdir::capability::MAIL_MESSAGE_ADD);
            assert_eq!(refusal.source, "msgraph-mail");
            assert!(refusal.detail.unwrap().contains("Drafts only"));
        }
        other => panic!("an add outside Drafts is refused at the gate: {other:?}"),
    }
    add("msgraph-mail/Drafts").expect("an add in Drafts is queued");
}

/// Gmail keeps no keyword but `$Important`, the IMPORTANT label.
///
/// Another is refused at the gate rather than queued and dropped by the
/// push, and `$Important` passes with no warning.
#[test]
fn a_gmail_source_refuses_any_keyword_but_important_at_the_gate() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("live")
        .for_source("gmail");
    store
        .ensure_collection("gmail/INBOX", "message/rfc822")
        .unwrap();
    store
        .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId("gmail/INBOX".into()),
            handle: PimdirHandle("m0".into()),
            link_id: Some(PimdirLinkId("m0@example.org".into())),
            object: None,
            level: PimdirLevel::Meta,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::Known(Default::default()),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: Some(PimdirBase {
                flags: PimdirFlags::Known(Default::default()),
                revision: None,
                object: None,
            }),
            origin: None,
        })])
        .unwrap();
    let config: AccountConfig = toml::from_str("gmail.auth.token.raw = \"token\"").unwrap();
    let mode = config.mode().unwrap();
    declare(dir.path(), "live", &config, &mode, &AccountState::default()).unwrap();
    let seq = store.list_items("gmail/INBOX", None, 1).unwrap()[0].seq;

    let set = |flag: &str| {
        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        let action = PimdirAction::SetFlags {
            seq,
            flags: PimdirFlags::from_iter([flag]),
            base: None,
        };
        producer.enqueue("gmail/INBOX", &action, None)
    };
    match set("$Custom") {
        Err(PimdirError::Unsupported(refusal)) => {
            assert_eq!(
                refusal.capability,
                io_pimdir::capability::MAIL_FLAGS_KEYWORDS
            );
            assert_eq!(refusal.source, "gmail");
            assert!(refusal.detail.unwrap().contains("but $Important"));
        }
        other => panic!("a keyword Gmail drops is refused at the gate: {other:?}"),
    }
    let important = set("$Important").expect("$Important is queued");
    assert!(important.partial.is_empty(), "{:?}", important.partial);
}

/// A calendar the target's server keeps from tasks refuses one.
///
/// A task in a hub calendar crosses to the target, so the refusal stands
/// under the source's declaration, the target undeclared; until every
/// target listed, the record keeps what the earlier run stated.
#[test]
fn a_target_s_events_only_calendar_refuses_a_task_before_it_is_queued() {
    let dir = tempfile::tempdir().unwrap();
    synced(dir.path(), "caldav", &["caldav/events", "caldav/work"]);
    synced(dir.path(), "dav", &["caldav/events", "caldav/work"]);
    let config: AccountConfig = toml::from_str(&format!(
        "{CALDAV}targets.dav.caldav.server = \"https://other.example.org/\"\n\
         targets.dav.caldav.auth.basic.username = \"user\"\n\
         targets.dav.caldav.auth.basic.password.raw = \"pw\""
    ))
    .unwrap();
    let mode = config.mode().unwrap();
    let declaring = Declaring {
        dir: dir.path(),
        account: "live",
        config: &config,
        mode: &mode,
    };

    let mut state = AccountState::default();
    let earlier = BTreeSet::from([String::from("caldav/gone")]);
    state.taskless.insert("caldav".into(), earlier);
    let mut taskless = PairTaskless::new(&state, "caldav");
    let source = [calendar("events", None), calendar("work", Some(true))];
    let target = [calendar("events", Some(false)), calendar("work", None)];
    record_pair_listing(
        &declaring,
        &mut state,
        "caldav",
        &mut taskless,
        [&source, &target],
        &[],
        false,
    );

    let refusal = refused_task(dir.path(), "caldav/events");
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
    assert_eq!(refusal.source, "caldav");
    queue_task(dir.path(), "caldav/work").unwrap();
    let reader = PimdirReader::open(dir.path()).unwrap();
    assert!(!reader.accepts_vtodo("caldav/events").unwrap());
    let syncing = reader.capabilities("caldav/events").unwrap();
    let target = syncing.iter().find(|source| source.source == "dav");
    assert!(
        target.unwrap().declared.is_none(),
        "a target stays undeclared"
    );

    let known = BTreeSet::from([String::from("caldav/events"), "caldav/gone".into()]);
    assert_eq!(state.taskless["caldav"], known);
}

/// A Google target keeps every calendar of the source it copies from
/// tasks, from the configuration alone.
#[test]
fn a_google_target_keeps_the_source_s_calendars_from_tasks() {
    let dir = tempfile::tempdir().unwrap();
    synced(dir.path(), "caldav", &["caldav/events"]);
    let config: AccountConfig = toml::from_str(&format!(
        "{CALDAV}targets.google.gcal.auth.token.raw = \"token\""
    ))
    .unwrap();
    let mode = config.mode().unwrap();
    declare(dir.path(), "live", &config, &mode, &AccountState::default()).unwrap();

    let refusal = refused_task(dir.path(), "caldav/events");
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
    assert_eq!(refusal.source, "caldav");
    assert!(refusal.detail.unwrap().contains("target google"));
}

/// A calendar a listing meets for the first time is gated from its
/// first item sync on, before the run's last declaration.
#[test]
fn a_calendar_met_mid_run_is_gated_from_its_first_item_sync() {
    let dir = tempfile::tempdir().unwrap();
    let config: AccountConfig = toml::from_str(CALDAV).unwrap();
    let mode = config.mode().unwrap();
    let declaring = Declaring {
        dir: dir.path(),
        account: "live",
        config: &config,
        mode: &mode,
    };
    let mut state = AccountState::default();
    declare(dir.path(), "live", &config, &mode, &state).unwrap();

    let store = open_store(dir.path(), "caldav", "live").unwrap();
    let listed = [
        calendar("events", Some(false)),
        calendar("work", Some(true)),
    ];
    let names = names_by_id(&listed);
    let kept = [String::from("events"), String::from("work")];
    let listing = Listing {
        listed: &listed,
        names: &names,
        kept: &kept,
        roles: None,
    };
    record_listing(
        &store,
        &declaring,
        &mut state,
        "caldav",
        "text/calendar",
        &listing,
        false,
    )
    .unwrap();
    drop(store);
    synced(dir.path(), "caldav", &["caldav/events", "caldav/work"]);

    let reader = PimdirReader::open(dir.path()).unwrap();
    assert!(!reader.accepts_vtodo("caldav/events").unwrap());
    assert!(reader.accepts_vtodo("caldav/work").unwrap());
    drop(reader);
    let refusal = refused_task(dir.path(), "caldav/events");
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
}

/// A Google calendar outside the home set, as io-webdav lists it: by
/// its path.
const GOOGLE_EVENTS: &str = "/caldav/v2/x%40g/events";

/// Records a local listing of `listed`, every collection kept, as
/// `run_local` does.
fn record_local(dir: &Path, state: &mut AccountState, listed: &[Collection]) {
    let config: AccountConfig = toml::from_str(CALDAV).unwrap();
    let mode = config.mode().unwrap();
    let declaring = Declaring {
        dir,
        account: "live",
        config: &config,
        mode: &mode,
    };
    let store = open_store(dir, "caldav", "live").unwrap();
    let names = names_by_id(listed);
    let kept: Vec<String> = listed.iter().map(|c| c.id.clone()).collect();
    let listing = Listing {
        listed,
        names: &names,
        kept: &kept,
        roles: None,
    };
    record_listing(
        &store,
        &declaring,
        state,
        "caldav",
        "text/calendar",
        &listing,
        false,
    )
    .unwrap();
}

/// A calendar listed and not synced yet refuses a task right after the
/// listing.
///
/// The source syncs it with no checkpoint, so its declaration holds
/// (pimdir STORAGE §15.6); one outside the home set has its path as id.
#[test]
fn a_listed_calendar_refuses_a_task_before_its_first_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AccountState::default();
    let listed = [
        calendar(GOOGLE_EVENTS, Some(false)),
        calendar("work", Some(true)),
    ];

    record_local(dir.path(), &mut state, &listed);

    let collection = format!("caldav/{GOOGLE_EVENTS}");
    let refusal = refused_task(dir.path(), &collection);
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
    assert_eq!(refusal.source, "caldav");
    queue_task(dir.path(), "caldav/work").unwrap();
    let reader = PimdirReader::open(dir.path()).unwrap();
    assert!(!reader.accepts_vtodo(&collection).unwrap());
    assert_eq!(reader.collection_sources(&collection).unwrap(), ["caldav"]);
    assert_eq!(state.taskless["caldav"], BTreeSet::from([collection]));
}

/// A listing records a source syncing a collection it holds no state
/// in, and leaves the checkpoint and coverage of one it does alone.
#[test]
fn a_listing_keeps_the_checkpoint_a_source_holds() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open_store(dir.path(), "caldav", "live").unwrap();
    store
        .ensure_collection("caldav/work", "text/calendar")
        .unwrap();
    store
        .write(vec![PimdirWriteOp::SetCheckpoint {
            collection: PimdirCollectionId("caldav/work".into()),
            checkpoint: PimdirCheckpoint(b"token-1".to_vec()),
        }])
        .unwrap();
    drop(store);

    let mut state = AccountState::default();
    let listed = [
        calendar("work", Some(true)),
        calendar("events", Some(false)),
    ];
    record_local(dir.path(), &mut state, &listed);
    record_local(dir.path(), &mut state, &listed);

    let store = open_store(dir.path(), "caldav", "live").unwrap();
    let load = |collection: &str| {
        store
            .load(
                &PimdirCollectionId(collection.into()),
                &PimdirLoadSelection::All,
            )
            .unwrap()
    };
    assert_eq!(
        load("caldav/work").checkpoint,
        Some(PimdirCheckpoint(b"token-1".to_vec())),
        "a checkpoint is never overwritten",
    );
    let events = load("caldav/events");
    assert_eq!(events.checkpoint, None, "no state, a first round");
    assert!(events.coverage.is_none() && events.round.is_none());
    let reader = PimdirReader::open(dir.path()).unwrap();
    assert_eq!(
        reader.collection_sources("caldav/events").unwrap(),
        ["caldav"]
    );
}

/// A target's events-only calendar refuses a task from the listing on.
///
/// A pair records its source as syncing the collections it is about to
/// sync; the target stays unrecorded and undeclared.
#[test]
fn a_pair_s_listed_calendar_refuses_a_task_before_its_first_sync() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path(), "caldav", "live").unwrap();
    for collection in ["caldav/events", "caldav/work"] {
        store
            .ensure_collection(collection, "text/calendar")
            .unwrap();
    }
    drop(store);
    let config: AccountConfig = toml::from_str(&format!(
        "{CALDAV}targets.dav.caldav.server = \"https://other.example.org/\"\n\
         targets.dav.caldav.auth.basic.username = \"user\"\n\
         targets.dav.caldav.auth.basic.password.raw = \"pw\""
    ))
    .unwrap();
    let mode = config.mode().unwrap();
    let declaring = Declaring {
        dir: dir.path(),
        account: "live",
        config: &config,
        mode: &mode,
    };

    let mut state = AccountState::default();
    let mut taskless = PairTaskless::new(&state, "caldav");
    let source = [calendar("events", None), calendar("work", None)];
    let target = [calendar("events", Some(false)), calendar("work", None)];
    let common = [String::from("caldav/events"), String::from("caldav/work")];
    record_pair_listing(
        &declaring,
        &mut state,
        "caldav",
        &mut taskless,
        [&source, &target],
        &common,
        false,
    );

    let refusal = refused_task(dir.path(), "caldav/events");
    assert_eq!(
        refusal.capability,
        io_pimdir::capability::CALENDAR_COMPONENT_VTODO
    );
    assert_eq!(refusal.source, "caldav");
    queue_task(dir.path(), "caldav/work").unwrap();
    let reader = PimdirReader::open(dir.path()).unwrap();
    assert_eq!(
        reader.collection_sources("caldav/events").unwrap(),
        ["caldav"]
    );
}

/// A configured filter names a calendar outside the home set by the id
/// it is listed under, its path.
#[test]
fn a_filter_names_a_calendar_by_its_path() {
    let collections = BTreeMap::from([
        (String::from(GOOGLE_EVENTS), String::from("Team")),
        (String::from("events"), String::from("Primary")),
    ]);

    let include = CollectionFilter::Include(vec![GOOGLE_EVENTS.into()]);
    assert_eq!(
        filter_collections(&collections, &include),
        BTreeSet::from([String::from(GOOGLE_EVENTS)])
    );
    let exclude = CollectionFilter::Exclude(vec![GOOGLE_EVENTS.into()]);
    assert_eq!(
        filter_collections(&collections, &exclude),
        BTreeSet::from([String::from("events")])
    );
    assert_eq!(
        wire_name("caldav", &hub_id("caldav", GOOGLE_EVENTS)),
        GOOGLE_EVENTS
    );
}

#[test]
fn the_retention_sweep_runs_only_when_a_delay_is_configured() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    let mut config: AccountConfig =
        toml::from_str(r#"imap.server = "imaps://imap.example.org:993""#).unwrap();

    let mut report = SyncOutput::default();
    sweep_retained(&config, &mut store, &mut report);
    assert!(report.purged.is_none());
    assert!(config.store.purge_cutoff(Utc::now()).is_none());

    config.store.purge_after = Some(std::time::Duration::from_secs(0));
    sweep_retained(&config, &mut store, &mut report);
    let purged = report.purged.expect("a retention section");
    assert_eq!(purged.items, 0);
    assert_eq!(purged.objects, 0);
    assert_eq!(purged.bytes, 0);
}

/// A purged item's body actually leaves the disk.
///
/// A purge releases a reference, not bytes, and the store collects nothing
/// (pimdir STORAGE §5), so a backup store grew while reporting reclaimed
/// bytes. A `0s` delay purges strictly before a millisecond cutoff.
#[test]
fn the_sweep_collects_the_bodies_the_purge_released() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = PimdirStore::open(dir.path()).unwrap().for_source("left");
    let hash = io_pimdir::hash::hash(b"abc");

    source
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: hash.clone(),
                    size: 3,
                },
                body: Some(b"abc".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("INBOX".into()),
                handle: PimdirHandle("1".into()),
                link_id: Some(PimdirLinkId("mid:gone@x".into())),
                object: Some(hash.clone()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: Some(hash.clone()),
                }),
                origin: None,
            }),
        ])
        .unwrap();

    let body = source.blobs().path(&hash).unwrap();
    assert!(body.is_file(), "the body was stored");

    source
        .write(vec![PimdirWriteOp::DropPlacement {
            collection: PimdirCollectionId("INBOX".into()),
            handle: PimdirHandle("1".into()),
            reason: PimdirDropReason::Deleted,
        }])
        .unwrap();
    assert!(body.is_file(), "retention keeps the body");

    thread::sleep(std::time::Duration::from_millis(5));

    let mut store = PimdirStore::open(dir.path()).unwrap();
    let config: AccountConfig = toml::from_str(
        r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0s"
            "#,
    )
    .unwrap();

    let mut report = SyncOutput::default();
    drop(source);
    sweep_retained(&config, &mut store, &mut report);

    let purged = report.purged.expect("a retention section");
    assert_eq!(purged.items, 1);
    assert_eq!(purged.objects, 1);
    assert_eq!(purged.bytes, 3);
    assert!(!body.exists(), "the body is gone from the blob tree");
}

/// Files `body` into `collection` of `account`, then drops it from its
/// source, which retains it (pimdir STORAGE §11).
fn retire(dir: &Path, account: &str, collection: &str, body: &[u8]) {
    let mut source = PimdirStore::open(dir)
        .unwrap()
        .for_account(account)
        .for_source(format!("{account}/imap"));
    source
        .ensure_collection(collection, "message/rfc822")
        .unwrap();
    let hash = io_pimdir::hash::hash(body);
    source
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: hash.clone(),
                    size: body.len(),
                },
                body: Some(body.to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId(collection.into()),
                handle: PimdirHandle("1".into()),
                link_id: Some(PimdirLinkId(format!("mid:{account}@x"))),
                object: Some(hash.clone()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: Some(hash),
                }),
                origin: None,
            }),
        ])
        .unwrap();
    source
        .write(vec![PimdirWriteOp::DropPlacement {
            collection: PimdirCollectionId(collection.into()),
            handle: PimdirHandle("1".into()),
            reason: PimdirDropReason::Deleted,
        }])
        .unwrap();
}

/// An account's `store.purge-after` decides for its own retained items.
///
/// The store's sweep is store-wide, so one account's `"0"` would empty
/// the trash of every account sharing the store.
#[test]
fn one_account_s_delay_purges_its_own_retained_items_alone() {
    let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    retire(dir.path(), "work", "work/imap/INBOX", b"work");
    retire(dir.path(), "home", "home/imap/INBOX", b"home");
    thread::sleep(std::time::Duration::from_millis(5));

    let config: AccountConfig = toml::from_str(
        r#"
            imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0s"
            "#,
    )
    .unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_account("work");
    let mut report = SyncOutput::default();
    sweep_retained(&config, &mut store, &mut report);

    assert_eq!(report.purged.expect("a retention section").items, 1);
    let gone = store.list_retained("work/imap/INBOX", None, 10).unwrap();
    assert!(gone.is_empty());
    let kept = store.list_retained("home/imap/INBOX", None, 10).unwrap();
    assert_eq!(kept.len(), 1, "the other account keeps its trash");
}

/// A mutable-content remote: every item carries an ETag.
///
/// The path no compiled-in backend exercises yet, proven here before any
/// DAV code exists, so a CardDAV failure later is a protocol bug rather
/// than an engine one.
struct MutableRemote {
    /// `handle -> (revision, accepted body)`.
    items: HashMap<String, (String, Option<PimdirHash>)>,
    /// The revision handed out by the next accepted write.
    next_revision: String,
    /// Every change this remote was asked to push, in order.
    pushed: Vec<PimdirChange>,
}

impl MutableRemote {
    fn at(handle: &str, revision: &str) -> Self {
        Self {
            items: HashMap::from([(handle.to_string(), (revision.to_string(), None))]),
            next_revision: String::from("v2"),
            pushed: Vec::new(),
        }
    }
}

impl PimdirRemote for MutableRemote {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let items = self
            .items
            .iter()
            .map(|(handle, (revision, _))| PimdirRemoteItem {
                handle: PimdirHandle(handle.clone()),
                flags: PimdirFlags::default(),
                revision: Some(revision.clone()),
                meta: named("uid:a"),
            })
            .collect();
        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
            items,
            Some(PimdirCheckpoint(b"token-1".to_vec())),
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        Ok(handles
            .into_iter()
            .filter_map(|handle| {
                let (revision, _) = self.items.get(&handle.0)?;
                Some(PimdirFetchedItem {
                    handle,
                    link_id: PimdirLinkId("uid:a".into()),
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    body: None,
                    revision: Some(revision.clone()),
                })
            })
            .collect())
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        let mut results = Vec::new();
        for change in changes {
            self.pushed.push(change.clone());
            let result = match &change.kind {
                PimdirChangeKind::Update {
                    handle,
                    object,
                    if_match,
                } => {
                    let current = self.items.get(&handle.0).map(|(rev, _)| rev.clone());
                    if if_match.is_some() && if_match.as_deref() == current.as_deref() {
                        let revision = self.next_revision.clone();
                        self.items
                            .insert(handle.0.clone(), (revision.clone(), Some(object.clone())));
                        PimdirPushResult {
                            handle: handle.clone(),
                            outcome: PimdirPushOutcome::Accepted,
                            assigned: None,
                            revision: Some(revision),
                            relocated: false,
                        }
                    } else {
                        PimdirPushResult {
                            handle: handle.clone(),
                            outcome: PimdirPushOutcome::Rejected,
                            assigned: None,
                            revision: None,
                            relocated: false,
                        }
                    }
                }
                other => anyhow::bail!("unexpected push: {other:?}"),
            };
            results.push(result);
        }
        Ok(results)
    }
}

/// The account a conflicted store is grouped, and counted, under.
const CONFLICT_ACCOUNT: &str = "cards";

/// Seeds one conflicted card with all three bodies present.
///
/// The state [`merge_conflicts`] starts from, so a test reaches it without
/// a server: what needs a connection is the fetch that lands the remote
/// body, and that is the other half of [`resolve_conflicts`].
fn store_with_conflict(
    dir: &std::path::Path,
    base: &str,
    local: &str,
    remote: &str,
) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir)
        .unwrap()
        .for_account(CONFLICT_ACCOUNT)
        .for_source("dav");
    store.ensure_collection("contacts", "text/vcard").unwrap();

    let blobs = store.blob_stage();
    let stored = |body: &str| PimdirWriteOp::StoreObject {
        object: PimdirObject {
            hash: blobs.blobs().hash(body.as_bytes()),
            size: body.len(),
        },
        body: Some(body.as_bytes().to_vec()),
    };

    store
        .write(vec![
            stored(base),
            stored(local),
            stored(remote),
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card1".into()),
                link_id: Some(PimdirLinkId("uid:a".into())),
                object: Some(blobs.blobs().hash(local.as_bytes())),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Conflict,
                conflict_revision: Some(String::from("etag-2")),
                conflict_object: Some(blobs.blobs().hash(remote.as_bytes())),
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(String::from("etag-1")),
                    object: Some(blobs.blobs().hash(base.as_bytes())),
                }),
                origin: None,
            }),
        ])
        .unwrap();

    store
}

/// A card carrying `tel` and `note`, the fields the merge tests move.
fn card(tel: &str, note: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:{tel}\r\nNOTE:{note}\r\nEND:VCARD\r\n"
    )
}

/// Two sides editing different fields have said nothing contradictory.
///
/// The base names which side touched which, both survive, the conflict
/// clears through the queue, and the run reports nothing. Asking a person
/// is a background tool asking to be switched off.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn disjoint_edits_on_both_sides_resolve_with_no_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_conflict(
        dir.path(),
        &card("+1", "old"),
        &card("+2", "old"),
        &card("+1", "new"),
    );
    let blobs = store.blob_stage();

    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "dav",
        &mut store,
        &blobs,
        dir.path(),
        &mut Noted::default(),
    )
    .unwrap();
    assert_eq!(merged, 1);

    let placement = load_side(&store, "contacts").unwrap().remove(0);
    assert_ne!(placement.status, PimdirStatus::Conflict);
    assert!(placement.conflict_object.is_none());
    assert_eq!(
        placement.base.and_then(|base| base.revision).as_deref(),
        Some("etag-2"),
        "the push the resolution stages is conditioned on the revision it \
         was merged against, not on the one the divergence started from",
    );

    let body = blobs
        .blobs()
        .get(&placement.object.unwrap())
        .unwrap()
        .unwrap();
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("TEL:+2"), "{body}");
    assert!(body.contains("NOTE:new"), "{body}");

    let mut report = SyncOutput::default();
    itemize_pulled(
        &[PimdirSyncEvent::Conflicted(PimdirHandle("card1".into()))],
        &HashMap::new(),
        &store,
        "contacts",
        "contacts",
        "dav",
        &mut report,
    )
    .unwrap();
    assert!(
        report.conflicts.is_empty(),
        "a divergence the run settled is not reported"
    );

    count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
    assert_eq!(report.outstanding_conflicts, 0);
}

/// A merge is counted by its own row, never by the totals of the drain,
/// which is the store's.
///
/// A frontend's resolution queued first lands first, so the merge, made over
/// the body it replaced, parks; the resolution is reported as applied.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_merge_is_judged_by_its_own_row_and_the_drain_reports_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let local = card("+2", "old");
    let mut store = store_with_conflict(dir.path(), &card("+1", "old"), &local, &card("+1", "new"));
    let blobs = store.blob_stage();

    let seq = store.seq_for_link("contacts", "uid:a").unwrap().unwrap();
    let local_hash = blobs.blobs().hash(local.as_bytes());
    let remote_hash = blobs.blobs().hash(card("+1", "new").as_bytes());
    let mut frontend = PimdirProducer::open(dir.path(), "cardamum").unwrap();
    frontend
        .enqueue_with_body("contacts", card("+3", "old").as_bytes(), |object| {
            PimdirAction::Update {
                seq,
                object,
                if_match: Some(local_hash),
                resolves: Some(vec![remote_hash]),
            }
        })
        .unwrap();
    drop(frontend);

    let mut noted = Noted::default();
    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "dav",
        &mut store,
        &blobs,
        dir.path(),
        &mut noted,
    )
    .unwrap();

    assert_eq!(
        merged, 0,
        "the merge parked behind the frontend's resolution"
    );
    let drained: Vec<_> = noted
        .drained
        .iter()
        .map(|entry| (entry.collection.as_str(), entry.applied))
        .collect();
    assert_eq!(drained, [("contacts", 1)]);
}

/// One card a fake CardDAV server holds under `card1`, its body read
/// with the listing as a DAV listing does.
#[cfg(any(feature = "carddav", feature = "caldav"))]
struct CardServer {
    revision: String,
    body: String,
    /// Whether the listing leaves the body out, to be fetched.
    bare: bool,
    /// How many bodies were fetched.
    fetched: usize,
}

#[cfg(any(feature = "carddav", feature = "caldav"))]
impl CardServer {
    fn at(revision: &str, body: &str) -> Self {
        Self {
            revision: revision.into(),
            body: body.into(),
            bare: false,
            fetched: 0,
        }
    }

    /// A server listing the card with no body, as a listing of
    /// metadata alone does.
    fn bare(revision: &str, body: &str) -> Self {
        Self {
            bare: true,
            ..Self::at(revision, body)
        }
    }

    fn item(&self) -> PimdirFetchedItem {
        let bytes = self.body.as_bytes().to_vec();
        PimdirFetchedItem {
            handle: PimdirHandle("card1".into()),
            link_id: PimdirLinkId("uid:a".into()),
            summary: None,
            sort_key: PimdirSortKey::default(),
            body: Some(PimdirFetchedBody::Inline {
                hash: io_pimdir::hash::hash(&bytes),
                bytes,
            }),
            revision: Some(self.revision.clone()),
        }
    }
}

#[cfg(any(feature = "carddav", feature = "caldav"))]
impl PimdirRemote for CardServer {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let item = self.item();
        let meta = match item.body {
            Some(body) if !self.bare => named("uid:a").with_body(body),
            _ => named("uid:a"),
        };
        let items = vec![PimdirRemoteItem {
            handle: item.handle,
            flags: PimdirFlags::default(),
            revision: item.revision,
            meta,
        }];
        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
            items, None,
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        _handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        self.fetched += 1;
        Ok(vec![self.item()])
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        panic!("a pulling pass pushes nothing: {changes:?}");
    }
}

/// One endpoint's pulling pass over `server`, as a reconcile's first
/// pass runs it.
#[cfg(any(feature = "carddav", feature = "caldav"))]
fn pull_endpoint(store: &mut PimdirSourceStore, server: &mut CardServer) -> PimdirSyncReport {
    let verb = sync_verb(
        "contacts",
        false,
        PimdirPushRights::all(),
        PimdirConflictPolicy::Manual,
        PimdirScope::unbounded(),
        false,
    );
    run_verb(store, server, verb).unwrap()
}

/// Two endpoints of one account over one store, `left` and `right`.
#[cfg(any(feature = "carddav", feature = "caldav"))]
fn two_endpoints(dir: &Path) -> (PimdirSourceStore, PimdirSourceStore) {
    let open = |source: &str| {
        PimdirStore::open(dir)
            .unwrap()
            .for_account(CONFLICT_ACCOUNT)
            .for_source(source)
    };
    let left = open("left");
    left.ensure_collection("contacts", "text/vcard").unwrap();
    (left, open("right"))
}

/// A divergence as its source and its base, local and remote bodies.
#[cfg(any(feature = "carddav", feature = "caldav"))]
type CardDivergence = (String, Option<String>, Option<String>, Option<String>);

/// The divergences the store holds for the card.
#[cfg(any(feature = "carddav", feature = "caldav"))]
fn card_divergence(store: &PimdirSourceStore, blobs: &PimdirBlobStage) -> Vec<CardDivergence> {
    let read = |hash: &Option<PimdirHash>| {
        let body = blobs.blobs().get(hash.as_ref()?).unwrap()?;
        Some(String::from_utf8(body).unwrap())
    };
    crate::conflict::list(store, CONFLICT_ACCOUNT)
        .unwrap()
        .into_iter()
        .map(|c| {
            (
                c.source.clone(),
                read(&c.base),
                read(&c.local),
                read(&c.remote),
            )
        })
        .collect()
}

/// A card both endpoints rewrote merges against the body they agreed on.
///
/// Each pass pulling only, as a reconcile's first does, the store keeps
/// that body and records the divergence (pimdir SYNC §9), so the merge
/// reads all three bodies and settles disjoint edits.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_card_both_endpoints_rewrote_diverges_against_their_agreed_body() {
    let dir = tempfile::tempdir().unwrap();
    let (mut left, mut right) = two_endpoints(dir.path());
    let blobs = left.blob_stage();
    let agreed = card("+1", "old");

    pull_endpoint(&mut left, &mut CardServer::at("v1", &agreed));
    pull_endpoint(&mut right, &mut CardServer::at("w1", &agreed));
    assert!(card_divergence(&left, &blobs).is_empty());

    pull_endpoint(&mut left, &mut CardServer::at("v2", &card("+2", "old")));
    pull_endpoint(&mut right, &mut CardServer::at("w2", &card("+1", "new")));

    assert_eq!(
        card_divergence(&left, &blobs),
        [(
            String::from("right"),
            Some(agreed),
            Some(card("+2", "old")),
            Some(card("+1", "new")),
        )]
    );

    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "right",
        &mut right,
        &blobs,
        dir.path(),
        &mut Noted::default(),
    )
    .unwrap();
    assert_eq!(merged, 1);
    assert!(card_divergence(&left, &blobs).is_empty());
    let item = load_side(&right, "contacts").unwrap().remove(0);
    let body = String::from_utf8(blobs.blobs().get(&item.object.unwrap()).unwrap().unwrap());
    let body = body.unwrap();
    assert!(
        body.contains("TEL:+2") && body.contains("NOTE:new"),
        "{body}"
    );
}

/// Two endpoints holding one card under two bodies before they ever
/// agreed park it with no ancestor (pimdir SYNC §6).
///
/// Neither body is taken over the other.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_card_two_endpoints_never_agreed_on_parks_with_no_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    let (mut left, mut right) = two_endpoints(dir.path());
    let blobs = left.blob_stage();

    pull_endpoint(&mut left, &mut CardServer::at("v1", &card("+1", "old")));
    pull_endpoint(&mut right, &mut CardServer::at("w1", &card("+9", "old")));

    assert_eq!(
        card_divergence(&left, &blobs),
        [(
            String::from("right"),
            None,
            Some(card("+1", "old")),
            Some(card("+9", "old")),
        )]
    );

    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "right",
        &mut right,
        &blobs,
        dir.path(),
        &mut Noted::default(),
    )
    .unwrap();
    assert_eq!(merged, 0, "nothing to merge against");
}

/// How a one-way target's upgrade settles what it fetches: its pushing
/// sync's policy.
const STORE_WINS: PimdirUpgradeOptions = PimdirUpgradeOptions {
    conflict: PimdirConflictPolicy::PreferLocal,
    push_content: true,
};

/// A side the store decides keeps the store's body over its own edit.
///
/// The engine parks a store-wins divergence on a pass that cannot push,
/// and the pushing pass never revisits a parked one, so the target kept its
/// edit and the run asked a person what `one-way` had already answered.
#[test]
fn a_side_the_store_decides_keeps_the_store_body_against_the_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_conflict(
        dir.path(),
        &card("+1", "old"),
        &card("+2", "old"),
        &card("+3", "old"),
    );
    let blobs = store.blob_stage();

    let mut remote = MutableRemote::at("card1", "etag-2");
    let kept = keep_store_bodies(
        "contacts",
        "dav",
        &mut store,
        &blobs,
        &mut remote,
        STORE_WINS,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(kept, 1);

    let placement = load_side(&store, "contacts").unwrap().remove(0);
    assert_eq!(placement.status, PimdirStatus::Dirty, "a push is owed");
    assert_eq!(
        placement.object,
        Some(blobs.blobs().hash(card("+2", "old").as_bytes()))
    );
    assert!(placement.conflict_revision.is_none());
    assert_eq!(
        placement.base.and_then(|base| base.revision).as_deref(),
        Some("etag-2"),
        "the overwrite is conditioned on the revision the endpoint holds",
    );
}

/// The fetch of a diverging body settles it by the side's policy (pimdir
/// SYNC §6); under the default options it settles nothing and parks.
///
/// A one-way target's `prefer-local` keeps the store's body, owed to the
/// target, and the target's own as a retained item reported overruled.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_fetched_divergence_settles_by_the_side_s_policy() {
    for (options, settles) in [(STORE_WINS, true), (PimdirUpgradeOptions::default(), false)] {
        let dir = tempfile::tempdir().unwrap();
        let (mut source, mut target) = two_endpoints(dir.path());
        let agreed = card("+1", "old");

        pull_endpoint(&mut source, &mut CardServer::at("v1", &agreed));
        pull_endpoint(&mut target, &mut CardServer::at("w1", &agreed));
        pull_endpoint(&mut source, &mut CardServer::at("v2", &card("+2", "old")));
        let verb = sync_verb(
            "contacts",
            false,
            PimdirPushRights::all(),
            PimdirConflictPolicy::PreferLocal,
            PimdirScope::unbounded(),
            false,
        );
        let edited = card("+3", "old");
        let parked = run_verb(&mut target, &mut CardServer::bare("w2", &edited), verb).unwrap();
        assert_eq!(parked.conflicts, 1, "the target's pull parked it");
        let unread: Vec<PimdirHandle> = conflicted_placements(&target, "contacts", "right")
            .unwrap()
            .into_iter()
            .filter(|placement| placement.conflict_object.is_none())
            .map(|placement| placement.handle)
            .collect();
        assert_eq!(unread, [PimdirHandle::from("card1")], "nobody read it");

        let mut server = CardServer::at("w2", &edited);
        let fetched = fetch_diverging(
            "contacts",
            "right",
            &mut target,
            &mut server,
            options,
            unread,
        )
        .unwrap();

        let overruled = fetched
            .events
            .iter()
            .filter(|event| matches!(event, PimdirSyncEvent::Overruled { .. }))
            .count();
        let placement = load_side(&target, "contacts").unwrap().remove(0);
        let retained = PimdirReader::open(dir.path())
            .unwrap()
            .list_retained("contacts", None, 10)
            .unwrap();
        if settles {
            assert_eq!(overruled, 1, "{:?}", fetched.events);
            assert_eq!(
                placement.status,
                PimdirStatus::Dirty,
                "the overwrite is owed"
            );
            assert!(placement.conflict_revision.is_none(), "{placement:?}");
            assert_eq!(
                retained
                    .iter()
                    .map(|item| item.object.clone())
                    .collect::<Vec<_>>(),
                [Some(target.blob_stage().blobs().hash(edited.as_bytes()))],
                "the target's own body kept",
            );
        } else {
            assert_eq!(overruled, 0, "{:?}", fetched.events);
            assert!(placement.conflict_revision.is_some(), "still parked");
            assert!(retained.is_empty(), "{retained:?}");
        }
    }
}

/// A one-way overwrite keeps the target's own edit as a retained item,
/// reported overruled: the body leaves its server, never the store.
///
/// The source moved an agreed card, the target edited it meanwhile, and
/// the target's pull, which may not push, parked the divergence.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_one_way_overwrite_keeps_the_target_body_as_a_retained_item() {
    let dir = tempfile::tempdir().unwrap();
    let (mut source, mut target) = two_endpoints(dir.path());
    let blobs = source.blob_stage();
    let agreed = card("+1", "old");

    pull_endpoint(&mut source, &mut CardServer::at("v1", &agreed));
    pull_endpoint(&mut target, &mut CardServer::at("w1", &agreed));
    pull_endpoint(&mut source, &mut CardServer::at("v2", &card("+2", "old")));
    let verb = sync_verb(
        "contacts",
        false,
        PimdirPushRights::all(),
        PimdirConflictPolicy::PreferLocal,
        PimdirScope::unbounded(),
        false,
    );
    let edited = card("+3", "old");
    let parked = run_verb(&mut target, &mut CardServer::at("w2", &edited), verb).unwrap();
    assert_eq!(parked.conflicts, 1, "the target's pull parked it");

    let mut overruled = Vec::new();
    let mut server = CardServer::at("w2", &edited);
    let kept = keep_store_bodies(
        "contacts",
        "right",
        &mut target,
        &blobs,
        &mut server,
        STORE_WINS,
        &mut overruled,
    );
    assert_eq!(kept.unwrap(), 1);

    let retained = PimdirReader::open(dir.path())
        .unwrap()
        .list_retained("contacts", None, 10)
        .unwrap();
    assert_eq!(retained.len(), 1, "{retained:?}");
    assert_eq!(
        retained[0].object,
        Some(blobs.blobs().hash(edited.as_bytes())),
        "the target's own body",
    );
    assert_eq!(
        overruled,
        [(
            String::from("contacts"),
            PimdirHandle::from("card1"),
            retained[0].link_id.clone(),
        )]
    );

    let placement = load_side(&target, "contacts").unwrap().remove(0);
    assert_eq!(
        placement.status,
        PimdirStatus::Dirty,
        "the overwrite is owed"
    );
    assert_eq!(
        placement.object,
        Some(blobs.blobs().hash(card("+2", "old").as_bytes()))
    );
    assert_eq!(
        placement.base.and_then(|base| base.revision).as_deref(),
        Some("w2"),
        "gated on the revision the target holds",
    );
}

/// A target edit listed with no body is fetched before the one-way
/// overwrite settles it, so the store keeps it as a retained item.
///
/// The store refuses to overrule a body nobody has read, and the run
/// takes that as a fetch owed, never as a failure.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_one_way_overwrite_fetches_an_unread_target_body_before_keeping_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut source, mut target) = two_endpoints(dir.path());
    let blobs = source.blob_stage();
    let agreed = card("+1", "old");

    pull_endpoint(&mut source, &mut CardServer::at("v1", &agreed));
    pull_endpoint(&mut target, &mut CardServer::at("w1", &agreed));
    pull_endpoint(&mut source, &mut CardServer::at("v2", &card("+2", "old")));
    let verb = sync_verb(
        "contacts",
        false,
        PimdirPushRights::all(),
        PimdirConflictPolicy::PreferLocal,
        PimdirScope::unbounded(),
        false,
    );
    let edited = card("+3", "old");
    let parked = run_verb(&mut target, &mut CardServer::bare("w2", &edited), verb).unwrap();
    assert_eq!(parked.conflicts, 1, "the target's pull parked it");
    let placement = load_side(&target, "contacts").unwrap().remove(0);
    assert_eq!(placement.conflict_object, None, "the target's body unread");

    let mut server = CardServer::bare("w2", &edited);
    let mut overruled = Vec::new();
    let kept = keep_store_bodies(
        "contacts",
        "right",
        &mut target,
        &blobs,
        &mut server,
        STORE_WINS,
        &mut overruled,
    );
    assert_eq!(kept.unwrap(), 1);
    assert_eq!(server.fetched, 1, "the target's body fetched first");

    let retained = PimdirReader::open(dir.path())
        .unwrap()
        .list_retained("contacts", None, 10)
        .unwrap();
    assert_eq!(retained.len(), 1, "{retained:?}");
    assert_eq!(
        retained[0].object,
        Some(blobs.blobs().hash(edited.as_bytes())),
        "the target's own body",
    );
    assert_eq!(
        overruled,
        [(
            String::from("contacts"),
            PimdirHandle::from("card1"),
            retained[0].link_id.clone(),
        )]
    );

    let placement = load_side(&target, "contacts").unwrap().remove(0);
    assert_eq!(placement.status, PimdirStatus::Dirty, "the overwrite owed");
    assert_eq!(
        placement.object,
        Some(blobs.blobs().hash(card("+2", "old").as_bytes()))
    );
}

/// A conflict the engine marked reaches the report the account prints.
///
/// The fold is [`SyncOutput::absorb`], the one a worker's collection report
/// travels back through. Merging only the item patch left the account
/// report with an empty `conflicts`, so nothing named what it had parked.
#[test]
fn a_conflict_the_engine_marked_survives_the_report_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_edit(dir.path(), "v1");
    let mut remote = MutableRemote::at("card1", "v9");

    let before = flag_snapshot(&store, "contacts", "left").unwrap();
    let sync = sync_with(&mut store, &mut remote, PimdirPushRights::all());
    assert_eq!(sync.conflicts, 1, "the engine parked it");

    let mut collection = SyncOutput::default();
    itemize_pulled(
        &sync.events,
        &before,
        &store,
        "contacts",
        "contacts",
        "left",
        &mut collection,
    )
    .unwrap();
    assert_eq!(collection.conflicts.len(), 1, "the collection names it");

    let mut account = SyncOutput::default();
    account.absorb(collection);
    assert_eq!(
        account.conflicts.len(),
        1,
        "and so does the account report, which is the one printed and the \
         one the notification is raised from",
    );

    let text = account.to_string();
    assert!(text.contains("Warnings (1)"), "{text}");
    assert!(text.contains("card1"), "{text}");
}

/// Both sides setting the same field is the residue no merge settles.
///
/// It stays parked and is counted from the store, the number the run's
/// exit code answers with 2; the printed account report is asserted, the
/// collection's alone having let the arm go missing.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn a_same_field_collision_parks_and_is_still_counted() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_conflict(
        dir.path(),
        &card("+1", "old"),
        &card("+2", "old"),
        &card("+3", "old"),
    );
    let blobs = store.blob_stage();
    let local = blobs.blobs().hash(card("+2", "old").as_bytes());

    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "dav",
        &mut store,
        &blobs,
        dir.path(),
        &mut Noted::default(),
    )
    .unwrap();
    assert_eq!(merged, 0);

    let placement = load_side(&store, "contacts").unwrap().remove(0);
    assert_eq!(placement.status, PimdirStatus::Conflict);
    assert_eq!(placement.object, Some(local), "the local side is untouched");

    let mut collection = SyncOutput::default();
    itemize_pulled(
        &[PimdirSyncEvent::Conflicted(PimdirHandle("card1".into()))],
        &HashMap::new(),
        &store,
        "contacts",
        "contacts",
        "dav",
        &mut collection,
    )
    .unwrap();
    assert_eq!(collection.conflicts.len(), 1);

    let mut report = SyncOutput::default();
    report.absorb(collection);
    assert_eq!(report.conflicts.len(), 1);

    count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
    assert_eq!(report.outstanding_conflicts, 1);
    assert!(report.left_waiting(), "which is what exits with 2");

    let text = report.to_string();
    assert!(text.contains("changed on both sides"), "{text}");
}

/// Two fields disagreeing is one item waiting, and two such items are two.
///
/// The merge counts colliding fields and the report counts parked items,
/// which are not the same number: a summary reading `1 item(s) waiting`
/// over a two-field divergence would have passed everything else.
#[cfg(any(feature = "carddav", feature = "caldav"))]
#[test]
fn two_colliding_fields_park_the_item_and_the_count_reads_past_one() {
    let dir = tempfile::tempdir().unwrap();
    let base = card("+1", "old");
    let local = card("+2", "mine");
    let remote = card("+3", "theirs");

    assert_eq!(
        Kind::Vcard.merge(base.as_bytes(), local.as_bytes(), remote.as_bytes()),
        Merged::Collided(2),
        "the phone number and the note each disagree, and the merge counts \
         both rather than stopping at the first",
    );

    let mut store = store_with_conflict(dir.path(), &base, &local, &remote);
    let blobs = store.blob_stage();

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId("contacts".into()),
            handle: PimdirHandle("card2".into()),
            link_id: Some(PimdirLinkId("uid:b".into())),
            object: Some(blobs.blobs().hash(local.as_bytes())),
            level: PimdirLevel::Full,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Conflict,
            conflict_revision: Some(String::from("etag-2")),
            conflict_object: Some(blobs.blobs().hash(remote.as_bytes())),
            base: Some(PimdirBase {
                flags: PimdirFlags::default(),
                revision: Some(String::from("etag-1")),
                object: Some(blobs.blobs().hash(base.as_bytes())),
            }),
            origin: None,
        })])
        .unwrap();

    let merged = merge_conflicts(
        "contacts",
        Kind::Vcard,
        "dav",
        &mut store,
        &blobs,
        dir.path(),
        &mut Noted::default(),
    )
    .unwrap();
    assert_eq!(
        merged, 0,
        "a body carrying a field nobody can pick is staged in part or not \
         at all, and it is not at all",
    );

    let parked = load_side(&store, "contacts").unwrap();
    assert_eq!(parked.len(), 2);
    for placement in &parked {
        assert_eq!(placement.status, PimdirStatus::Conflict);
        assert_eq!(
            placement.object,
            Some(blobs.blobs().hash(local.as_bytes())),
            "the local side of {} is untouched",
            placement.handle.0,
        );
    }

    let mut collection = SyncOutput::default();
    itemize_pulled(
        &[
            PimdirSyncEvent::Conflicted(PimdirHandle("card1".into())),
            PimdirSyncEvent::Conflicted(PimdirHandle("card2".into())),
        ],
        &HashMap::new(),
        &store,
        "contacts",
        "contacts",
        "dav",
        &mut collection,
    )
    .unwrap();

    let mut report = SyncOutput::default();
    report.absorb(collection);
    assert_eq!(
        report.conflicts.len(),
        2,
        "each item is named once, however many of its fields collided",
    );

    count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
    assert_eq!(report.outstanding_conflicts, 2);
    assert!(report.left_waiting(), "which is what exits with 2");

    let text = report.to_string();
    assert!(text.contains("Warnings (2)"), "{text}");
    assert!(
        text.contains("Conflicts: 2 item(s) waiting for a decision"),
        "{text}",
    );
}

/// A write the remote would not take is not a write the run made.
///
/// The item patch is the plan, derived before anything is pushed, so a
/// refused `PUT` used to be counted among the hunks the run applied:
/// `synchronized: 1 hunks`, exit 0, and the same phantom on every run.
#[test]
fn a_refused_write_is_reported_instead_of_counted_as_a_hunk() {
    let mut report = SyncOutput::default();
    for id in ["card1", "card2"] {
        report.item_patch.push(PatchEntry::new(
            ItemHunk::Update {
                side: String::from("dav"),
                collection: String::from("Default"),
                id: String::from(id),
                content_key: 0,
            },
            None,
        ));
    }

    itemize_rejected(
        "dav",
        vec![RejectedPush {
            collection: String::from("Default"),
            handle: String::from("card1"),
            action: "update",
            failure: Failure::from("HTTP 403: Resource is not a vCard object"),
        }],
        &mut report,
    );

    let ids: Vec<&ItemHunk> = report.item_patch.iter().map(|entry| &entry.hunk).collect();
    assert_eq!(ids.len(), 1, "the refused write left the patch: {ids:?}");
    assert!(format!("{:?}", ids[0]).contains("card2"), "{ids:?}");

    let text = report.to_string();
    assert!(text.contains("Warnings (1)"), "{text}");
    assert!(text.contains("refused the update of card1"), "{text}");
    assert!(text.contains("synchronized: 1 hunks"), "{text}");
    assert_eq!(
        Exit::from(&report),
        Exit::Conflicted,
        "a run that could not deliver a write is not a run that succeeded",
    );
}

/// A refused write keeps its code and its causes: JSON carries them
/// apart, the text prints them as one chain.
#[test]
fn a_refused_write_keeps_its_code_and_causes() {
    let mut report = SyncOutput::default();
    itemize_rejected(
        "dav",
        vec![RejectedPush {
            collection: String::from("Default"),
            handle: String::from("card1"),
            action: "update",
            failure: Failure {
                code: Some(crate::error::ErrorCode::PermissionDenied),
                error: String::from("Cannot update card1 in Default"),
                sources: vec![String::from("WebDAV server returned HTTP 403")],
            },
        }],
        &mut report,
    );

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(
        json["rejected"],
        serde_json::json!([{
            "side": "dav",
            "collection": "Default",
            "id": "card1",
            "kind": "update",
            "code": "permission-denied",
            "error": "Cannot update card1 in Default",
            "sources": ["WebDAV server returned HTTP 403"],
        }])
    );
    let text = report.to_string();
    let chain = "Cannot update card1 in Default: WebDAV server returned HTTP 403";
    assert!(text.contains(chain), "{text}");
}

/// A refused append names the item and takes its hunk back.
///
/// A create is staged under its provisional handle, U+0001 then the link
/// id (pimdir SYNC §2). Read as a marker after the link id, it reported
/// an empty id and left the copy, or a frontend's add, in the patch.
#[test]
fn a_refused_append_is_named_by_its_link_id_and_its_hunk_is_taken_back() {
    let mut report = SyncOutput::default();
    for id in ["event-1", "event-2"] {
        report.item_patch.push(PatchEntry::new(
            ItemHunk::Copy {
                source_side: String::from("local"),
                target_side: String::from("dav"),
                collection: String::from("Calendar"),
                source_id: String::from(id),
                flags: BTreeSet::new(),
                origin: None,
                content_key: 0,
            },
            None,
        ));
    }
    report.item_patch.push(PatchEntry::new(
        ItemHunk::Add {
            side: String::from("dav"),
            collection: String::from("Calendar"),
            id: String::from("event-3"),
            flags: BTreeSet::new(),
            content_key: 0,
        },
        None,
    ));

    let rejected = |link: &str| RejectedPush {
        collection: String::from("Calendar"),
        handle: PimdirLinkId::from(link).provisional().0,
        action: "append",
        failure: Failure::from("HTTP 403"),
    };
    itemize_rejected(
        "dav",
        vec![rejected("event-1"), rejected("event-3")],
        &mut report,
    );

    let left: Vec<&ItemHunk> = report.item_patch.iter().map(|entry| &entry.hunk).collect();
    assert_eq!(left.len(), 1, "a refused append left the patch: {left:?}");
    assert!(format!("{:?}", left[0]).contains("event-2"), "{left:?}");

    let ids: Vec<Option<&str>> = report
        .rejected
        .iter()
        .map(|rejected| rejected.failure.id.as_deref())
        .collect();
    assert_eq!(ids, [Some("event-1"), Some("event-3")]);
    let text = report.to_string();
    assert!(text.contains("refused the append of event-1 in"), "{text}");
    assert!(!text.contains('\u{1}'), "the marker is printed raw");
}

/// A conflict an earlier run parked reaches no later run's report.
///
/// The engine returns early for a placement it already marked, so there is
/// nothing to announce. The store still holds the decision, which is why
/// the two numbers are read from two places.
#[test]
fn a_second_run_over_an_unresolved_conflict_announces_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_conflict(
        dir.path(),
        &card("+1", "old"),
        &card("+2", "old"),
        &card("+3", "old"),
    );

    let mut report = SyncOutput::default();
    itemize_pulled(
        &[],
        &HashMap::new(),
        &store,
        "contacts",
        "contacts",
        "dav",
        &mut report,
    )
    .unwrap();
    assert!(report.conflicts.is_empty());

    count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
    assert_eq!(report.outstanding_conflicts, 1);

    let text = report.to_string();
    assert!(text.contains("1 item(s) waiting for a decision"), "{text}");
}

/// A queued `move` of an item in conflict is parked, never applied
/// (pimdir SYNC §7, STORAGE §15.2).
///
/// The report names the row and the diverging body, the card stays where
/// it stood and its decision still waits.
#[test]
fn a_queued_move_over_a_conflict_parks_and_the_decision_still_waits() {
    let dir = tempfile::tempdir().unwrap();
    let remote = card("+3", "old");
    let mut store =
        store_with_conflict(dir.path(), &card("+1", "old"), &card("+2", "old"), &remote);
    store.ensure_collection("archive", "text/vcard").unwrap();
    let seq = store.seq_for_link("contacts", "uid:a").unwrap().unwrap();
    let diverging = store.blob_stage().blobs().hash(remote.as_bytes());

    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    let action = PimdirAction::Move {
        seq,
        to: PimdirCollectionId("archive".into()),
    };
    producer.enqueue("contacts", &action, None).unwrap();
    drop(producer);

    let mut report = SyncOutput::default();
    drain_queues(&mut store, &mut report);
    report_parked(&store, CONFLICT_ACCOUNT, &mut report);
    count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);

    assert!(report.drained.is_empty(), "{:?}", report.drained);
    assert_eq!(report.parked.len(), 1);
    let parked = &report.parked[0];
    assert_eq!(parked.kind, "move");
    assert!(parked.error.contains(&diverging.0), "{}", parked.error);
    assert_eq!(report.outstanding_conflicts, 1);

    let held = load_side(&store, "contacts").unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].status, PimdirStatus::Conflict);
    assert!(load_side(&store, "archive").unwrap().is_empty());
}

/// A create a frontend staged must still read back as a create.
///
/// The engine pushes an add only for a `Created` placement, so a status the
/// store cannot hold back never reaches the server. A two-replica account
/// looped against Fastmail so: six staged cards, six hunks, none received.
#[test]
fn a_queued_create_reads_back_as_a_create() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account(CONFLICT_ACCOUNT)
        .for_source("dav");
    store
        .ensure_collection("dav/contacts", "text/vcard")
        .unwrap();

    let body = card("+1", "new");
    let object = store.blob_stage().stage_body(body.as_bytes()).unwrap();
    let hash = object.hash.clone();

    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    producer
        .enqueue(
            "dav/contacts",
            &PimdirAction::Add {
                link_id: Some(PimdirLinkId("uid:new".into())),
                flags: PimdirFlags::default(),
                object: Some(hash),
                supersedes: None,
            },
            Some(&object),
        )
        .unwrap();
    drop(producer);

    assert_eq!(store.drain().unwrap().applied, 1);

    let placement = load_side(&store, "dav/contacts").unwrap().remove(0);
    assert_eq!(
        placement.status,
        PimdirStatus::Created,
        "a staged create the store reads back as {:?} is never pushed",
        placement.status,
    );
}

/// An edit over a base naming a revision and no body is owed that body:
/// the engine pushes it only once a fetch reads the base (pimdir SYNC §5).
#[test]
fn an_edit_over_an_unread_base_is_owed_its_base_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account(CONFLICT_ACCOUNT)
        .for_source("dav");
    store
        .ensure_collection("dav/contacts", "text/vcard")
        .unwrap();
    let object = store
        .blob_stage()
        .stage_body(card("+2", "edited").as_bytes())
        .unwrap();
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: object.clone(),
                body: None,
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("dav/contacts".into()),
                handle: PimdirHandle::from("card1"),
                link_id: Some(PimdirLinkId("uid:card1".into())),
                object: Some(object.hash.clone()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Dirty,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some("r1".into()),
                    object: None,
                }),
                origin: None,
            }),
        ])
        .unwrap();

    let collections = vec![String::from("dav/contacts")];
    let plans = hydrate_plans(&store, "dav", &collections, None, &GoneBodies::default()).unwrap();
    let owed: Vec<_> = plans[0].1.iter().map(|target| &target.handle).collect();
    assert_eq!(owed, [&PimdirHandle::from("card1")]);
}

/// The name of the item's original body, `old`: a real one, a body
/// being verified against its name before it is pushed.
fn old_hash() -> PimdirHash {
    io_pimdir::hash::hash(b"old")
}

/// The name of the item's edited body, `new`.
fn new_hash() -> PimdirHash {
    io_pimdir::hash::hash(b"new")
}

/// Seeds a store holding one locally edited item.
///
/// Its body points at `edited`, its base at `original` and
/// `base_revision`: what a frontend leaves after staging an edit offline.
fn store_with_local_edit(dir: &std::path::Path, base_revision: &str) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir).unwrap().for_source("left");
    store.ensure_collection("contacts", "text/vcard").unwrap();
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: old_hash(),
                    size: 3,
                },
                body: Some(b"old".to_vec()),
            },
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: new_hash(),
                    size: 3,
                },
                body: Some(b"new".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card1".into()),
                link_id: Some(PimdirLinkId("uid:a".into())),
                object: Some(new_hash()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Dirty,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(base_revision.to_string()),
                    object: Some(old_hash()),
                }),
                origin: None,
            }),
        ])
        .unwrap();
    store
}

/// One side of a two-endpoint account, holding one locally edited card.
///
/// Both sides hold the same item under the same link id, which is what an
/// account mirroring two servers is: one hub item, one binding per
/// endpoint, each with a base of its own.
fn endpoint_with_local_edit(
    dir: &std::path::Path,
    source: &str,
    base_revision: &str,
) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir).unwrap().for_source(source);
    store.ensure_collection("contacts", "text/vcard").unwrap();
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: old_hash(),
                    size: 3,
                },
                body: Some(b"old".to_vec()),
            },
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: new_hash(),
                    size: 3,
                },
                body: Some(b"new".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card1".into()),
                link_id: Some(PimdirLinkId("uid:a".into())),
                object: Some(new_hash()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Dirty,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(base_revision.to_string()),
                    object: Some(old_hash()),
                }),
                origin: None,
            }),
        ])
        .unwrap();
    store
}

/// Puts a parked divergence back the way this run's own merge leaves it.
///
/// Dirty again, rebased on the revision it conflicted at, so the next pass
/// pushes and can meet a remote that moved once more.
fn settle_as_the_merge_would(store: &mut PimdirSourceStore) {
    let mut placement = load_side(store, "contacts").unwrap().remove(0);
    let revision = placement.conflict_revision.take();

    placement.status = PimdirStatus::Dirty;
    placement.conflict_object = None;
    placement.base = Some(PimdirBase {
        flags: PimdirFlags::default(),
        revision,
        object: Some(old_hash()),
    });

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
        .unwrap();
}

/// A source and a target both park a divergence, each named once.
///
/// `reconcile_pass` asked whether anything moved and threw the events away,
/// so a two-endpoint account reported no conflict anywhere. Naive itemizing
/// double-counts instead, which is why this drives more than one pass.
#[test]
fn a_conflict_on_each_endpoint_is_named_once_across_a_convergence_loop() {
    let dir = tempfile::tempdir().unwrap();
    let mut left = endpoint_with_local_edit(dir.path(), "left", "l1");
    let mut right = endpoint_with_local_edit(dir.path(), "right", "r1");
    let mut left_remote = MutableRemote::at("card1", "l9");
    let mut right_remote = MutableRemote::at("card1", "r9");

    let mut report = SyncOutput::default();
    for pass in 0..2 {
        let left_pass = sync_with(&mut left, &mut left_remote, PimdirPushRights::all());
        itemize_conflicted(
            &left_pass.events,
            &left,
            "contacts",
            "contacts",
            "left",
            &mut report,
        )
        .unwrap();

        let right_pass = sync_with(&mut right, &mut right_remote, PimdirPushRights::all());
        itemize_conflicted(
            &right_pass.events,
            &right,
            "contacts",
            "contacts",
            "right",
            &mut report,
        )
        .unwrap();

        assert_eq!(
            (pass, left_pass.conflicts),
            (pass, 1),
            "the engine parked the left side on pass {pass}",
        );

        settle_as_the_merge_would(&mut left);
        left_remote
            .items
            .insert(String::from("card1"), (String::from("l10"), None));
    }

    assert_eq!(
        report.conflicts.len(),
        2,
        "one line per endpoint, not one per pass: {:?}",
        report.conflicts,
    );
    let sides: Vec<&str> = report
        .conflicts
        .iter()
        .map(|conflict| conflict.side.as_str())
        .collect();
    assert_eq!(sides, ["left", "right"]);

    assert_eq!(
        warn_conflicts(&report),
        2,
        "and one announcement per item entering conflict",
    );

    let mut later = SyncOutput::default();
    for (store, remote, side) in [
        (&mut left, &mut left_remote, "left"),
        (&mut right, &mut right_remote, "right"),
    ] {
        let pass = sync_with(store, remote, PimdirPushRights::all());
        itemize_conflicted(
            &pass.events,
            store,
            "contacts",
            "contacts",
            side,
            &mut later,
        )
        .unwrap();
    }

    assert!(later.conflicts.is_empty());
    assert_eq!(
        warn_conflicts(&later),
        0,
        "a conflict an earlier run parked is announced by no later one",
    );
}

/// One side of a two-endpoint account, holding one reconciled card.
///
/// Clean against its own server at `revision`, so what a pull round finds
/// is the server's own doing and nothing this run staged.
fn endpoint_holding(dir: &std::path::Path, source: &str, revision: &str) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir).unwrap().for_source(source);
    store.ensure_collection("contacts", "text/vcard").unwrap();
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: old_hash(),
                    size: 3,
                },
                body: Some(b"old".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card1".into()),
                link_id: Some(PimdirLinkId("uid:a".into())),
                object: Some(old_hash()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(revision.to_string()),
                    object: Some(old_hash()),
                }),
                origin: None,
            }),
        ])
        .unwrap();
    store
}

/// A server's own delete is named once on each side.
///
/// The two-endpoint path read only conflicts out of its pass, so a card a
/// server dropped reached the report as the propagation alone. Itemizing
/// a pushing pass instead double-counts, so the opening round only pulls.
#[test]
fn a_server_delete_is_named_once_on_each_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let mut left = endpoint_holding(dir.path(), "left", "l1");
    let right = endpoint_holding(dir.path(), "right", "r1");

    let mut left_remote = MutableRemote::at("card1", "l1");
    left_remote.items.clear();
    let opts = |accept_absences| PimdirSyncOptions {
        push: false,
        accept_absences,
        ..Default::default()
    };

    let parked = run_verb(
        &mut left,
        &mut left_remote,
        sync_verb_with("contacts", opts(false), true),
    )
    .unwrap();
    assert_eq!(
        parked.parked,
        Some(PimdirParked {
            absent: 1,
            bound: 1
        })
    );
    assert!(parked.events.is_empty(), "{:?}", parked.events);

    let before = flag_snapshot(&left, "contacts", "left").unwrap();
    let pull = run_verb(
        &mut left,
        &mut left_remote,
        sync_verb_with("contacts", opts(true), true),
    )
    .unwrap();
    assert!(
        pull.events
            .iter()
            .any(|event| matches!(event, PimdirSyncEvent::Vanished(_))),
        "the engine saw the card go: {:?}",
        pull.events,
    );

    let mut report = SyncOutput::default();
    itemize_pulled(
        &pull.events,
        &before,
        &left,
        "contacts",
        "contacts",
        "left",
        &mut report,
    )
    .unwrap();

    for (source, other) in [("left", "right"), ("right", "left")] {
        for placement in projection_view(&right, "contacts", source).unwrap() {
            for hunk in placement_hunks(source, other, source, "contacts", &placement) {
                report.item_patch.push(PatchEntry::new(hunk, None));
            }
        }
    }

    let deletes: Vec<(&str, Direction)> = report
        .item_patch
        .iter()
        .filter_map(|entry| match &entry.hunk {
            ItemHunk::Delete {
                side, direction, ..
            } => Some((side.as_str(), *direction)),
            _ => None,
        })
        .collect();
    assert_eq!(
        deletes,
        [("left", Direction::Pull), ("right", Direction::Push)],
        "the pull names it on left, the projection owes it on right: {:?}",
        report.item_patch,
    );

    let json = serde_json::to_string(&report.item_patch).unwrap();
    assert!(
        json.contains(r#""side":"left","collection":"contacts","id":"card1","direction":"pull""#),
        "{json}"
    );
    let text = report.to_string();
    assert!(
        text.contains("pull the deletion of item card1 in contacts from left"),
        "{text}"
    );
    assert!(
        text.contains("delete item card1 in contacts on right"),
        "{text}"
    );
}

/// A store holding the tombstone a frontend leaves after an offline delete.
fn store_with_local_delete(dir: &std::path::Path) -> PimdirSourceStore {
    let mut store = PimdirStore::open(dir).unwrap().for_source("left");
    store.ensure_collection("contacts", "text/vcard").unwrap();
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: old_hash(),
                    size: 3,
                },
                body: Some(b"old".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card1".into()),
                link_id: Some(PimdirLinkId("uid:a".into())),
                object: Some(old_hash()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Tombstone,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(String::from("v1")),
                    object: Some(old_hash()),
                }),
                origin: None,
            }),
        ])
        .unwrap();
    store
}

fn sync_with(
    store: &mut PimdirSourceStore,
    remote: &mut MutableRemote,
    rights: PimdirPushRights,
) -> PimdirSyncReport {
    run_verb(
        store,
        remote,
        sync_verb(
            "contacts",
            true,
            rights,
            PimdirConflictPolicy::Manual,
            PimdirScope::unbounded(),
            true,
        ),
    )
    .unwrap()
}

/// A side that may not delete holds the tombstone instead of undoing it.
///
/// Both refusals run through one disposition, and for a hub-backed store it
/// has to be `Keep`: undoing the delete says the source still holds the
/// member, so the hub clears the deletion and the item returns everywhere.
#[test]
fn a_side_that_may_not_delete_keeps_the_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_delete(dir.path());
    let mut remote = MutableRemote::at("card1", "v1");

    let rights = PimdirPushRights {
        remove: false,
        ..PimdirPushRights::all()
    };
    let report = sync_with(&mut store, &mut remote, rights);

    assert_eq!(report.pushed, 0, "the delete is not pushed");
    assert!(remote.pushed.is_empty(), "and nothing else is either");

    let placements = store
        .load(
            &PimdirCollectionId("contacts".into()),
            &PimdirLoadSelection::All,
        )
        .unwrap()
        .placements;
    assert_eq!(placements.len(), 1);
    assert_eq!(
        placements[0].status,
        PimdirStatus::Tombstone,
        "the removal is still a removal, not undone into a clean row",
    );
}

/// A flag on a card or a calendar item is the store's alone (a `pimdir
/// tag`); mail keeps the right.
///
/// It is never pushed, so the backend rejects and warns nothing on every
/// run, and the flag stays.
#[test]
fn a_store_only_flag_on_a_card_stays_local() {
    let perms = SourcePermissions {
        collection: Default::default(),
        flag: Default::default(),
        item: Default::default(),
    };
    assert!(push_rights(&perms, "message/rfc822").flags);
    assert!(!push_rights(&perms, "text/calendar").flags);

    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_delete(dir.path());
    let contacts = PimdirCollectionId("contacts".into());
    let mut placement = store
        .load(&contacts, &PimdirLoadSelection::All)
        .unwrap()
        .placements
        .remove(0);
    placement.status = PimdirStatus::Dirty;
    placement.flags = PimdirFlags::from_iter(["$pinned"]);
    store
        .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
        .unwrap();
    let mut remote = MutableRemote::at("card1", "v1");

    for _ in 0..2 {
        sync_with(&mut store, &mut remote, push_rights(&perms, "text/vcard"));
    }

    assert!(remote.pushed.is_empty(), "no flag change pushed");
    let placements = store
        .load(&contacts, &PimdirLoadSelection::All)
        .unwrap()
        .placements;
    assert_eq!(
        placements[0].flags,
        PimdirFlags::from_iter(["$pinned"]),
        "the flag kept in the store"
    );
}

#[test]
fn a_local_body_edit_is_pushed_conditionally_and_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_edit(dir.path(), "v1");
    let mut remote = MutableRemote::at("card1", "v1");

    let report = sync_with(&mut store, &mut remote, PimdirPushRights::all());

    assert!(
        matches!(
            remote.pushed.as_slice(),
            [PimdirChange { kind: PimdirChangeKind::Update { if_match, .. }, .. }]
                if if_match.as_deref() == Some("v1")
        ),
        "expected one If-Match update, got {:?}",
        remote.pushed
    );
    assert_eq!(report.conflicts, 0);
    assert_eq!(report.rejected, 0);

    assert_eq!(
        remote.items["card1"],
        (String::from("v2"), Some(new_hash()))
    );
    let placements = load_side(&store, "contacts").unwrap();
    assert_eq!(placements[0].status, PimdirStatus::Clean);
    let base = placements[0].base.as_ref().expect("a rebased base");
    assert_eq!(base.revision.as_deref(), Some("v2"));
    assert_eq!(base.object, Some(new_hash()));
}

#[test]
fn a_body_edited_on_both_sides_is_left_conflicted_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_edit(dir.path(), "v1");
    let mut remote = MutableRemote::at("card1", "v9");

    let report = sync_with(&mut store, &mut remote, PimdirPushRights::all());

    assert_eq!(
        remote.items["card1"].1, None,
        "remote must not be clobbered"
    );
    assert_eq!(report.conflicts, 1);
    assert!(
        report
            .events
            .iter()
            .any(|e| matches!(e, PimdirSyncEvent::Conflicted(h) if h.0 == "card1")),
        "expected a Conflicted event, got {:?}",
        report.events
    );

    let placements = load_side(&store, "contacts").unwrap();
    assert_eq!(
        placements[0].status,
        PimdirStatus::Conflict,
        "the conflict must survive the round trip through the store"
    );
    assert_eq!(
        placements[0].conflict_revision.as_deref(),
        Some("v9"),
        "the conflicting remote revision must survive with it"
    );
}

#[test]
fn a_side_denied_item_update_keeps_the_edit_pending() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with_local_edit(dir.path(), "v1");
    let mut remote = MutableRemote::at("card1", "v1");

    let rights = PimdirPushRights {
        content: false,
        ..PimdirPushRights::all()
    };
    sync_with(&mut store, &mut remote, rights);

    assert!(
        remote.pushed.is_empty(),
        "a forbidden update must not reach the remote, got {:?}",
        remote.pushed
    );
    let placements = load_side(&store, "contacts").unwrap();
    assert_eq!(placements[0].status, PimdirStatus::Dirty);
    assert_eq!(placements[0].object, Some(new_hash()));
}

/// A `Meta`-level placement with a base, as after a first reconcile,
/// summarised with the octet length a relay needs.
fn linked(handle: &str, link: &str, size: Option<u64>) -> PimdirPlacement {
    PimdirPlacement {
        collection: PimdirCollectionId("INBOX".into()),
        handle: PimdirHandle(handle.into()),
        link_id: Some(PimdirLinkId(link.into())),
        object: None,
        level: PimdirLevel::Meta,
        summary: Some(PimdirSummary::Mail(PimdirMailSummary {
            size,
            ..Default::default()
        })),
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::default(),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        base: Some(PimdirBase {
            flags: PimdirFlags::default(),
            revision: None,
            object: None,
        }),
        origin: None,
    }
}

/// Two copies of one identity are both ordinary items.
///
/// Where the freeze this reverses left one of them out of the store and
/// warned, the second now carries a minted key, projects a copy like any
/// other item, and the run says nothing of its own about the pair.
#[test]
fn a_duplicated_identity_is_two_items_and_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    store.ensure_collection("INBOX", "message/rfc822").unwrap();

    store
        .write(vec![
            PimdirWriteOp::UpsertPlacement(linked("145", "a@x", None)),
            PimdirWriteOp::UpsertPlacement(linked("146", "dup:a@x#146", None)),
        ])
        .unwrap();

    let view = projection_view(&store, "INBOX", "left").unwrap();
    assert_eq!(view.len(), 2, "both copies are stored and projected");

    let mut report = SyncOutput {
        account: "dup".into(),
        ..Default::default()
    };
    for placement in &view {
        for hunk in placement_hunks("left", "right", "left", "INBOX", placement) {
            report.item_patch.push(PatchEntry::new(hunk, None));
        }
    }

    let text = report.to_string();
    assert!(!text.contains("Warnings"), "{text}");

    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("ambiguous").is_none(), "{json}");
    assert_eq!(json["duplicates"], serde_json::json!([]), "{json}");
}

/// A target refusing the second copy is the one thing worth a line.
///
/// The run wrote nothing for that item, and the line carries the `UID` and
/// the collection the user repairs it by.
#[test]
fn a_refused_duplicate_is_named_with_its_uid() {
    let refused = vec![RefusedCreate {
        collection: String::from("agenda"),
        uid: String::from("event-1@google.com"),
        handle: String::from("copy-1.ics"),
    }];

    let mut report = SyncOutput {
        account: "dup".into(),
        ..Default::default()
    };
    itemize_refused("right", refused, &mut report);

    assert_eq!(report.duplicates.len(), 1);
    assert_eq!(report.duplicates[0].side, "right");

    let text = report.to_string();
    assert!(text.contains("Warnings (1)"), "{text}");
    assert!(text.contains("agenda"), "{text}");
    assert!(text.contains("event-1@google.com"), "{text}");

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["duplicates"][0]["side"], "right");
    assert_eq!(json["duplicates"][0]["collection"], "agenda");
    assert_eq!(json["duplicates"][0]["uid"], "event-1@google.com");
}

/// A refusal is reported once however many passes met it.
///
/// A collection reconciles until quiescent, so the create is re-derived and
/// answered the same way each pass. Two copies of one identity are still
/// two refusals, which is the distinction the handle keeps.
#[test]
fn a_refusal_re_observed_on_a_later_pass_is_still_one_refusal() {
    let refused = |handle: &str| RefusedCreate {
        collection: String::from("agenda"),
        uid: String::from("event-1@google.com"),
        handle: String::from(handle),
    };

    let mut report = SyncOutput::default();
    itemize_refused(
        "right",
        vec![
            refused("copy-1.ics"),
            refused("copy-1.ics"),
            refused("copy-2.ics"),
        ],
        &mut report,
    );

    assert_eq!(
        report.duplicates.len(),
        2,
        "one line per copy, however many passes met the same answer",
    );
}

/// The same for a rejected write: re-derived every pass, reported once.
#[test]
fn a_write_refused_on_every_pass_is_still_one_line() {
    let rejected = |handle: &str| RejectedPush {
        collection: String::from("Default"),
        handle: String::from(handle),
        action: "update",
        failure: Failure::from("HTTP 403: Resource is not a vCard object"),
    };

    let mut report = SyncOutput::default();
    report.item_patch.push(PatchEntry::new(
        ItemHunk::Update {
            side: String::from("dav"),
            collection: String::from("Default"),
            id: String::from("card1"),
            content_key: 0,
        },
        None,
    ));
    itemize_rejected(
        "dav",
        vec![rejected("card1"), rejected("card1"), rejected("card2")],
        &mut report,
    );

    assert_eq!(report.rejected.len(), 2);
    assert!(report.item_patch.is_empty(), "and the hunk is still gone");
}

#[test]
fn a_relayed_copy_is_itemized_so_the_run_never_reads_as_already_in_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
    store.ensure_collection("INBOX", "message/rfc822").unwrap();

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(linked(
            "1",
            "mid:a@x",
            Some(42),
        ))])
        .unwrap();
    let mut right = PimdirStore::open(dir.path()).unwrap().for_source("right");
    right.write(covered(None)).unwrap();

    let targets = relay_targets(
        &store,
        Kind::Mail,
        "INBOX",
        ("left", false),
        ("right", true),
    )
    .unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].holding, "left");
    assert_eq!(targets[0].link, "mid:a@x");

    let target = &targets[0];
    let mut report = SyncOutput {
        account: "relay".into(),
        ..Default::default()
    };
    report.item_patch.push(PatchEntry::new(
        ItemHunk::Copy {
            source_side: target.holding.clone(),
            target_side: String::from("right"),
            collection: "INBOX".into(),
            source_id: target.link.clone(),
            flags: target.flags.iter().cloned().collect(),
            origin: None,
            content_key: content_key(&target.link),
        },
        None,
    ));

    let text = report.to_string();
    assert!(!text.contains("already in sync"), "{text}");
    assert!(text.contains("synchronized: 1 hunks"), "{text}");
    assert!(text.contains("from left to right"), "{text}");
}

/// A run whose every hunk failed, offline say, synchronized nothing, and
/// one that applied some counts those alone.
#[test]
fn a_run_whose_every_hunk_failed_is_not_synchronized() {
    let scan = |side: &str| CollectionHunk::Scan {
        side: side.into(),
        collection: "*".into(),
    };
    let mut report = SyncOutput {
        account: "a".into(),
        ..Default::default()
    };
    for side in ["imap", "carddav"] {
        let error = anyhow!("Network is unreachable");
        report
            .collection_patch
            .push(PatchEntry::new(scan(side), Some(error)));
    }

    let text = report.to_string();
    assert!(
        text.contains("Account a not synchronized: 2 errors\n"),
        "{text}"
    );

    report
        .collection_patch
        .push(PatchEntry::new(scan("caldav"), None));
    let text = report.to_string();
    assert!(
        text.contains("Account a partially synchronized: 1 hunks, 2 errors\n"),
        "{text}"
    );
}

/// A store whose `left` holds `mid:a@x` dated `date`, the `right`
/// target's coverage of the inbox written by `right`.
fn relay_store(dir: &Path, date: Option<&str>, right: Vec<PimdirWriteOp>) -> PimdirStore {
    let mut placement = linked("1", "mid:a@x", Some(42));
    placement.summary = Some(PimdirSummary::Mail(PimdirMailSummary {
        size: Some(42),
        date: date.map(String::from),
        ..Default::default()
    }));
    let mut left = PimdirStore::open(dir).unwrap().for_source("left");
    left.ensure_collection("INBOX", "message/rfc822").unwrap();
    left.write(vec![PimdirWriteOp::UpsertPlacement(placement)])
        .unwrap();
    let mut target = PimdirStore::open(dir).unwrap().for_source("right");
    target.write(right).unwrap();
    PimdirStore::open(dir).unwrap()
}

/// The writes of a source that closed a round over the inbox covering
/// from `since`.
fn covered(since: Option<&str>) -> Vec<PimdirWriteOp> {
    let scope = PimdirScope {
        since: since.map(String::from),
        until: None,
    };
    vec![
        PimdirWriteOp::OpenRound {
            collection: PimdirCollectionId("INBOX".into()),
            scope: scope.clone(),
            band: false,
        },
        PimdirWriteOp::CloseRound {
            collection: PimdirCollectionId("INBOX".into()),
            coverage: scope,
            checkpoint: Some(PimdirCheckpoint(b"c".to_vec())),
        },
    ]
}

/// The links a relay would copy into `right`.
fn relayed(store: &PimdirStore) -> Vec<String> {
    relay_targets(store, Kind::Mail, "INBOX", ("left", false), ("right", true))
        .unwrap()
        .into_iter()
        .map(|target| target.link)
        .collect()
}

/// A relay appends only where the target's listing is closed.
///
/// Before the target closed a round, or while one is open, the store
/// cannot tell whether it already holds the message (pimdir SYNC §5).
#[test]
fn a_relay_waits_for_the_target_to_close_a_round() {
    let never = tempfile::tempdir().unwrap();
    assert!(relayed(&relay_store(never.path(), None, vec![])).is_empty());

    let open = tempfile::tempdir().unwrap();
    let opening = PimdirWriteOp::OpenRound {
        collection: PimdirCollectionId("INBOX".into()),
        scope: PimdirScope::unbounded(),
        band: false,
    };
    let store = relay_store(open.path(), None, [covered(None), vec![opening]].concat());
    assert!(relayed(&store).is_empty());

    let closed = tempfile::tempdir().unwrap();
    let store = relay_store(closed.path(), None, covered(None));
    assert_eq!(relayed(&store), ["mid:a@x"]);
}

/// A message dated outside the target's covered scope waits: the
/// target never listed there; an undated one is in every scope.
#[test]
fn a_relay_keeps_to_the_target_s_scope() {
    let since = Some("2026-10-01T00:00:00Z");

    let old = tempfile::tempdir().unwrap();
    let store = relay_store(old.path(), Some("2026-01-01T00:00:00Z"), covered(since));
    assert!(relayed(&store).is_empty());

    let recent = tempfile::tempdir().unwrap();
    let store = relay_store(recent.path(), Some("2026-10-20T00:00:00Z"), covered(since));
    assert_eq!(relayed(&store), ["mid:a@x"]);

    let undated = tempfile::tempdir().unwrap();
    let store = relay_store(undated.path(), None, covered(since));
    assert_eq!(relayed(&store), ["mid:a@x"]);
}

/// What keeps a relay from repeating itself.
///
/// A relay bypasses the engine, so nothing else writes the copy down: an
/// item is selected for being on one source alone, and only the target's
/// binding takes it out. Without it every pass relays the same body again.
#[test]
fn a_relayed_copy_is_bound_to_its_target_and_never_relayed_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");
    left.ensure_collection("INBOX", "message/rfc822").unwrap();

    left.write(vec![PimdirWriteOp::UpsertPlacement(linked(
        "1",
        "mid:a@x",
        Some(42),
    ))])
    .unwrap();
    PimdirStore::open(dir.path())
        .unwrap()
        .for_source("right")
        .write(covered(None))
        .unwrap();

    let sides = |store: &PimdirStore| {
        relay_targets(store, Kind::Mail, "INBOX", ("left", false), ("right", true)).unwrap()
    };
    let targets = sides(&left);
    assert_eq!(targets.len(), 1);

    let mut right = PimdirStore::open(dir.path()).unwrap().for_source("right");
    let written = WrittenItem {
        id: String::from("77"),
        revision: None,
    };
    bind_relayed(&mut right, "INBOX", &targets[0], written).unwrap();

    assert!(sides(&right).is_empty(), "the copy is not relayed again");

    let hub = right.load_hub("INBOX").unwrap();
    let item = hub.items.get(&PimdirLinkId::from("mid:a@x")).unwrap();
    assert_eq!(item.sources.len(), 2, "the item is two-sided now");

    let binding = &item.sources[&source_id("right")];
    assert_eq!(binding.handle, PimdirHandle::from("77"), "as assigned");
    assert!(
        binding.base.is_some(),
        "and based, so the next sync reads it as in sync rather than as a create",
    );
    assert!(item.object.is_none(), "the relay stored no body");
    assert_eq!(item.level, PimdirLevel::Meta, "so it claims no more");
}

/// A message of `octets` octets stating `message_id`.
#[cfg(feature = "imap")]
fn sized_message(message_id: &str, octets: usize) -> Vec<u8> {
    let mut body = format!("Message-ID: <{message_id}>\r\nSubject: relayed\r\n\r\n").into_bytes();
    while body.len() < octets {
        body.extend_from_slice(b"relayed line of text\r\n");
    }
    body.truncate(octets);
    body
}

/// Two IMAP sides over `holding` and `target`, renamed `left` and `right`,
/// and a store relaying `left`'s UIDs to `right`, each listed at its size.
#[cfg(feature = "imap")]
fn imap_relay(
    dir: &Path,
    holding: &crate::imap::fake::FakeImap,
    target: &crate::imap::fake::FakeImap,
    listed: &[(&str, &str, usize)],
) -> (SourceCtx, SourceCtx, PimdirSourceStore, PimdirSourceStore) {
    let (mut left, _) = bound_imap_inbox(&dir.join("l"), holding, true, None);
    let (mut right, _) = bound_imap_inbox(&dir.join("r"), target, true, None);
    left.name = String::from("left");
    right.name = String::from("right");

    let placements = listed
        .iter()
        .map(|(uid, link, size)| {
            PimdirWriteOp::UpsertPlacement(linked(uid, link, Some(*size as u64)))
        })
        .collect();
    let mut left_store = PimdirStore::open(dir).unwrap().for_source("left");
    left_store
        .ensure_collection("INBOX", "message/rfc822")
        .unwrap();
    left_store.write(placements).unwrap();
    let mut right_store = PimdirStore::open(dir).unwrap().for_source("right");
    right_store.write(covered(None)).unwrap();

    (left, right, left_store, right_store)
}

/// An empty fake mailbox naming what it is appended.
#[cfg(feature = "imap")]
fn empty_imap() -> crate::imap::fake::FakeImap {
    use crate::imap::fake::{FakeImap, FakeMailbox};

    let server = FakeImap::start(
        FakeMailbox {
            uid_validity: 1,
            highest_mod_seq: 1,
            messages: Vec::new(),
            states_exists: true,
        },
        false,
    );
    server.keep_appends();
    server
}

/// A target refusing the append of a message larger than the pipe at its
/// command line fails the relay of that message rather than hanging it.
#[test]
#[cfg(feature = "imap")]
fn a_relay_the_target_refuses_at_once_returns() {
    use std::{sync::mpsc, time::Duration};

    let body = sized_message("m1@x", 600_000);
    let holding = one_message(1, 1);
    holding.set_header(1, &body);
    let target = empty_imap();
    target.refuse_next_append_line("NO [TOOBIG] too big");

    let dir = tempfile::tempdir().unwrap();
    let (mut left, mut right, left_store, _) = imap_relay(
        dir.path(),
        &holding,
        &target,
        &[("1", "mid:m1@x", body.len())],
    );
    let targets = relay_targets(
        &left_store,
        Kind::Mail,
        "INBOX",
        ("left", false),
        ("right", true),
    )
    .unwrap();

    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        let relayed = relay_one(&mut left.pool, &mut right.pool, "INBOX", &targets[0]);
        sent.send(relayed.map(|written| written.map(|w| w.id)))
            .unwrap();
    });
    let relayed = received
        .recv_timeout(Duration::from_secs(30))
        .expect("the relay returns");

    let err = relayed.unwrap().unwrap_err();
    assert!(format!("{err:#}").contains("too big"), "{err:#}");
}

/// A body longer than its listed size is never appended cut, and the side
/// it failed on serves the next relay.
#[test]
#[cfg(feature = "imap")]
fn a_body_longer_than_listed_is_not_appended_cut() {
    let body = sized_message("m1@x", 2_000);
    let holding = one_message(1, 1);
    holding.set_header(1, &body);
    let target = empty_imap();

    let dir = tempfile::tempdir().unwrap();
    let listed = [("1", "mid:m1@x", body.len() - 10)];
    let (mut left, mut right, left_store, _) = imap_relay(dir.path(), &holding, &target, &listed);
    let targets = relay_targets(
        &left_store,
        Kind::Mail,
        "INBOX",
        ("left", false),
        ("right", true),
    )
    .unwrap();

    let relayed = relay_one(&mut left.pool, &mut right.pool, "INBOX", &targets[0]).unwrap();
    let err = relayed.map(|w| w.id).unwrap_err();
    assert!(err.is::<SizeMismatch>(), "{err:#}");

    let dir = tempfile::tempdir().unwrap();
    let (_, _, left_store, _) = imap_relay(
        dir.path(),
        &holding,
        &target,
        &[("1", "mid:m1@x", body.len())],
    );
    let targets = relay_targets(
        &left_store,
        Kind::Mail,
        "INBOX",
        ("left", false),
        ("right", true),
    )
    .unwrap();
    let relayed = relay_one(&mut left.pool, &mut right.pool, "INBOX", &targets[0]).unwrap();
    assert!(relayed.is_ok(), "the same sides relay again: {relayed:?}");
}

/// A message its target refuses is reported rejected, and the relay goes on
/// with the next one.
#[test]
#[cfg(feature = "imap")]
fn a_refused_relay_is_reported_and_the_next_message_relayed() {
    use crate::imap::fake::{FakeImap, FakeMailbox, FakeMessage};

    let message = |uid: u32, id: &str| FakeMessage {
        uid,
        flags: Vec::new(),
        message_id: String::from(id),
    };
    let holding = FakeImap::start(
        FakeMailbox {
            uid_validity: 1,
            highest_mod_seq: 9,
            messages: vec![message(1, "m1@x"), message(2, "m2@x")],
            states_exists: true,
        },
        false,
    );
    let first = sized_message("m1@x", 500);
    let second = sized_message("m2@x", 500);
    holding.set_header(1, &first);
    holding.set_header(2, &second);
    let target = empty_imap();
    target.refuse_next_write("NO [OVERQUOTA] full");

    let dir = tempfile::tempdir().unwrap();
    let listed = [("1", "mid:m1@x", 500), ("2", "mid:m2@x", 500)];
    let (mut left, mut right, mut left_store, mut right_store) =
        imap_relay(dir.path(), &holding, &target, &listed);
    let progress = CollectionProgress {
        progress: Progress::default(),
        gate: PercentGate::default(),
        source: "left",
        target: "right",
        collection: "INBOX",
    };
    let mut report = SyncOutput::default();

    let relayed = relay_copies(
        "INBOX",
        &mut left,
        &mut right,
        &mut left_store,
        &mut right_store,
        &progress,
        &mut report,
    )
    .unwrap();

    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert_eq!(relayed, 1);
    assert_eq!(report.item_patch.len(), 1);
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert_eq!(report.rejected[0].kind, "append");
    assert_eq!(report.rejected[0].side, "right");
    assert_eq!(Exit::from(&report), Exit::Conflicted);
}

/// A hub collection id carries its namespace and the wire name does not.
///
/// Getting this backwards is how a mailbox and an address book both called
/// `Default` would end up as one collection.
#[test]
fn a_hub_id_carries_its_namespace_and_the_display_name_does_not() {
    assert_eq!(hub_id("mail", "INBOX"), "mail/INBOX");
    assert_eq!(display_name("mail", "mail/INBOX"), "INBOX");

    assert_eq!(hub_id("mail", "Archive/2026"), "mail/Archive/2026");
    assert_eq!(display_name("mail", "mail/Archive/2026"), "Archive/2026");
    assert_eq!(display_name("mail", "mailbox/INBOX"), "mailbox/INBOX");
    assert_eq!(display_name("cards", "mail/INBOX"), "mail/INBOX");
}

/// `--source` picks sources, there being no namespace to pick instead.
#[test]
fn narrowing_selects_the_named_sources() {
    let account: AccountConfig = toml::from_str(
        r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.b.imap.server = "imaps://b.example.org:993"
            sources.c.imap.server = "imaps://c.example.org:993"
            "#,
    )
    .unwrap();
    let mode = account.mode().unwrap();

    let picked = select_sources(&mode, &[String::from("a")]).unwrap();
    assert_eq!(picked, vec!["a"]);

    assert_eq!(select_sources(&mode, &[]).unwrap().len(), 3);

    let err = select_sources(&mode, &[String::from("nope")])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no source named nope"), "got {err}");
}

/// Stages one `submit` in the store at `dir`, anchored on `collection`,
/// for `source` to send.
fn queue_submit(dir: &Path, collection: &str, source: &str) {
    let payload = format!(r#"{{"v":1,"source":"{source}","from":"a@x.org","rcpts":["b@y.org"]}}"#);
    PimdirProducer::open(dir, "test-frontend")
        .unwrap()
        .enqueue(
            collection,
            &PimdirAction::Unknown {
                kind: submit::SUBMIT.into(),
                payload,
                object_hash: None,
            },
            None,
        )
        .unwrap();
}

/// An intent of a version this neverest does not know stays pending and
/// untouched for one that knows it (pimdir STORAGE §15.3).
///
/// No performer lists it, so it is neither performed nor parked as
/// malformed; one stating no version is still listed, to be parked.
#[test]
fn an_intent_of_an_unknown_version_stays_pending() {
    let dir = tempfile::tempdir().unwrap();
    let store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
    store
        .ensure_collection("imap/Sent", "message/rfc822")
        .unwrap();
    let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    let mut queue = |kind: &str, payload: &str| {
        let action = PimdirAction::Unknown {
            kind: kind.into(),
            payload: payload.into(),
            object_hash: None,
        };
        producer.enqueue("imap/Sent", &action, None).unwrap();
    };
    queue(
        submit::SUBMIT,
        r#"{"v":2,"source":"imap","to":["b@y.org"]}"#,
    );
    queue(invitation::REPLY, r#"{"v":2,"source":"imap"}"#);
    queue(
        "collection-create",
        r#"{"v":2,"source":"imap","title":"A"}"#,
    );

    assert!(submit::pending(&store).unwrap().is_empty());
    assert!(invitation::pending(&store).unwrap().is_empty());
    assert!(collection::pending(&store).unwrap().is_empty());
    assert_eq!(store.list_pending_actions().unwrap().len(), 3, "untouched");
    assert!(store.parked_actions().unwrap().is_empty());

    for payload in [r#"{"source":"imap"}"#, r#"{"v":"2","source":"imap"}"#] {
        assert!(
            !crate::sync::states_unknown_version(submit::SUBMIT, payload),
            "{payload} names no version to wait for: it parks"
        );
    }
}

/// A send an earlier run left in flight is parked unconfirmed, pimdir's
/// state for an unknown outcome (STORAGE §15.2), and its mark cleared.
///
/// Its producer reads `Unconfirmed` with what neverest recorded, where a
/// prefix of neverest's own read as any parked row.
#[test]
fn an_interrupted_send_is_parked_unconfirmed() {
    use io_pimdir::client::producer::PimdirActionStatus;

    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
    store
        .ensure_collection("imap/Sent", "message/rfc822")
        .unwrap();
    queue_submit(dir.path(), "imap/Sent", "imap");
    let intents = submit::pending(&store).unwrap();
    let id = intents[0].id;
    let in_flight = submit::InFlight::new(dir.path());
    in_flight.begin(id).unwrap();

    let mut report = SyncOutput::default();
    let left = park_interrupted(&mut store, &in_flight, intents, &mut report);

    assert_eq!(left.map(|intents| intents.len()), Some(0), "not sent again");
    assert_eq!(in_flight.left(), None);
    let entry = &report.submitted[0];
    assert!(entry.parked && entry.unconfirmed);
    let error = entry.error.as_deref().unwrap();
    assert!(error.starts_with("unconfirmed: "), "{error}");

    let producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
    let PimdirActionStatus::Unconfirmed { detail, .. } = producer.action_status(id).unwrap() else {
        panic!("the row reads as an intent whose outcome is unknown");
    };
    assert_eq!(error, format!("unconfirmed: {detail}"));
    assert!(
        submit::pending(&store).unwrap().is_empty(),
        "no run lists it"
    );
}

/// A run performing intents alone opens an endpoint for a send only
/// when the endpoint sends it natively: an `smtp` table opens alone.
#[test]
fn a_send_needs_its_source_opened_only_when_it_sends_natively() {
    let smtp: AccountConfig = toml::from_str(
        r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtps://smtp.example.org:465"
            "#,
    )
    .unwrap();
    let graph: AccountConfig = toml::from_str(
        r#"
            msgraph.auth.token.raw = "tok"
            msgraph.mail = true
            "#,
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
    store
        .ensure_collection("imap/Sent", "message/rfc822")
        .unwrap();
    queue_submit(dir.path(), "imap/Sent", "imap");
    assert!(wanted_sides(&store, &smtp, &["imap"], "imap").is_empty());

    let dir = tempfile::tempdir().unwrap();
    let store = PimdirStore::open(dir.path())
        .unwrap()
        .for_source("msgraph-mail");
    store
        .ensure_collection("msgraph-mail/Sent Items", "message/rfc822")
        .unwrap();
    assert!(wanted_sides(&store, &graph, &["msgraph-mail"], "msgraph-mail").is_empty());
    queue_submit(dir.path(), "msgraph-mail/Sent Items", "msgraph-mail");
    assert_eq!(
        wanted_sides(&store, &graph, &["msgraph-mail"], "msgraph-mail"),
        BTreeSet::from([String::from("msgraph-mail")])
    );
}

/// The authority is the whole of `one-way`.
///
/// It is what stops a divergence becoming a conflict nobody can resolve.
#[test]
fn the_authority_decides_the_conflict_and_the_push() {
    assert_eq!(
        Authority::Shared.conflict_policy(),
        PimdirConflictPolicy::Manual,
    );
    assert!(
        Authority::Shared.writes_back(),
        "both sides write in a two-way mirror",
    );

    assert_eq!(
        Authority::Endpoint.conflict_policy(),
        PimdirConflictPolicy::PreferRemote,
    );
    assert!(
        !Authority::Endpoint.writes_back(),
        "what the source holds is the truth, so nothing is written back to it",
    );

    assert_eq!(
        Authority::Store.conflict_policy(),
        PimdirConflictPolicy::PreferLocal,
    );
    assert!(
        Authority::Store.writes_back(),
        "the target takes what the store pushes",
    );
}

/// A kind named by its body arrives with it, its page fetching the bodies.
///
/// The plan read off the store alone would call it done, so a first sync of
/// a whole address book reported nothing at all: the pull's events name it.
#[test]
fn the_pull_plan_names_an_item_its_listing_brought_with_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
    store
        .ensure_collection("dav/contacts", "text/vcard")
        .unwrap();

    let object = io_pimdir::hash::hash(b"abc");
    store
        .write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: object.clone(),
                    size: 3,
                },
                body: Some(b"abc".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("dav/contacts".into()),
                handle: PimdirHandle("card-1.vcf".into()),
                link_id: Some(PimdirLinkId("uid:card-1".into())),
                object: Some(object.clone()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(String::from("etag-1")),
                    object: Some(object),
                }),
                origin: None,
            }),
        ])
        .unwrap();

    let mut report = SyncOutput::default();
    itemize_fetches(
        "dav/contacts",
        "contacts",
        &store,
        "dav",
        &[],
        &HashSet::new(),
        &mut report,
    )
    .unwrap();
    assert!(
        report.item_patch.is_empty(),
        "a body held from before is no fetch"
    );

    let added = [PimdirSyncEvent::Added(PimdirHandle("card-1.vcf".into()))];
    itemize_fetches(
        "dav/contacts",
        "contacts",
        &store,
        "dav",
        &added,
        &HashSet::new(),
        &mut report,
    )
    .unwrap();
    assert_eq!(report.item_patch.len(), 1);
    assert!(
        matches!(&report.item_patch[0].hunk, ItemHunk::Fetch { id, .. } if id == "uid:card-1"),
        "named by the identity its body states",
    );
}

/// A content change drops the stale object and leaves the level alone.
///
/// A plan keyed on the level therefore calls an item about to be re-fetched
/// done.
#[test]
fn the_pull_plan_names_a_body_dropped_by_a_content_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
    store
        .ensure_collection("dav/contacts", "text/vcard")
        .unwrap();

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId("dav/contacts".into()),
            handle: PimdirHandle("card-1.vcf".into()),
            link_id: Some(PimdirLinkId("uid:card-1".into())),
            object: None,
            level: PimdirLevel::Full,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: None,
            origin: None,
        })])
        .unwrap();

    let mut report = SyncOutput::default();
    itemize_fetches(
        "dav/contacts",
        "contacts",
        &store,
        "dav",
        &[],
        &HashSet::new(),
        &mut report,
    )
    .unwrap();

    assert_eq!(
        report.item_patch.len(),
        1,
        "Full with no object is a body to re-fetch, not a finished item",
    );
}

/// A calendar whose server implements no `sync-collection`.
///
/// Every enumeration is the whole member set, and an identity resolves only
/// by downloading the body. The reported Posteo shape, one `UID` under two
/// hrefs.
struct FullListingRemote {
    /// `handle -> (uid, body)`, in the order the listing returns them.
    items: Vec<(String, String, Vec<u8>)>,
    /// Every handle a body was fetched for, so a re-download is visible.
    fetched: Vec<String>,
    /// What the store binds, `handle -> link id`, which names a listed
    /// member at the same revision without its body, as `PimRemote` does.
    bound: HashMap<String, String>,
}

impl PimdirRemote for FullListingRemote {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let mut items = Vec::new();
        for (handle, uid, body) in self.items.clone() {
            let meta = match self.bound.get(&handle) {
                Some(link) => named(link),
                None => {
                    self.fetched.push(handle.clone());
                    named(&uid).with_body(PimdirFetchedBody::Inline {
                        hash: io_pimdir::hash::hash(&body),
                        bytes: body,
                    })
                }
            };
            items.push(PimdirRemoteItem {
                handle: PimdirHandle(handle),
                flags: PimdirFlags::from_iter([] as [String; 0]),
                revision: Some(String::from("etag-1")),
                meta,
            });
        }
        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
            items,
            Some(PimdirCheckpoint(Vec::new())),
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        let mut items = Vec::new();
        for handle in handles {
            let Some((_, uid, body)) = self
                .items
                .iter()
                .find(|(id, _, _)| *id == handle.0)
                .cloned()
            else {
                continue;
            };
            self.fetched.push(handle.0.clone());
            items.push(PimdirFetchedItem {
                handle,
                link_id: PimdirLinkId(uid),
                summary: None,
                sort_key: PimdirSortKey::default(),
                body: Some(PimdirFetchedBody::Inline {
                    hash: io_pimdir::hash::hash(&body),
                    bytes: body,
                }),
                revision: Some(String::from("etag-1")),
            });
        }
        Ok(items)
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        _changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        anyhow::bail!("the listing remote is never pushed to")
    }
}

/// One collection's spine as `collection_spine` runs it for a lone source.
///
/// Pull, the listing bringing the bodies of what the store does not bind,
/// which is where a DAV identity resolves, then report what came.
fn spine(
    store: &mut PimdirSourceStore,
    remote: &mut FullListingRemote,
    collection: &str,
) -> SyncOutput {
    remote.bound = load_side(store, collection)
        .unwrap()
        .into_iter()
        .filter(|p| p.base.is_some())
        .filter_map(|p| Some((p.handle.0, p.link_id?.0)))
        .collect();

    let mut report = SyncOutput::default();
    let pull = run_verb(
        store,
        remote,
        sync_verb(
            collection,
            false,
            PimdirPushRights::all(),
            PimdirConflictPolicy::Manual,
            PimdirScope::unbounded(),
            true,
        ),
    )
    .unwrap();
    itemize_fetches(
        collection,
        "agenda",
        store,
        "caldav",
        &pull.events,
        &HashSet::new(),
        &mut report,
    )
    .unwrap();
    report
}

/// One `UID` under two hrefs mirrors as two items; the next run is silent.
///
/// That second run is the bug the user saw: the frozen twin never got a
/// row, so the pull plan read it as an unfetched body and named it every
/// run. The collection is listed in full here, as that server lists it.
#[test]
fn one_identity_under_two_hrefs_mirrors_as_two_items_and_settles() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
    store
        .ensure_collection("caldav/agenda", "text/calendar")
        .unwrap();

    let uid = "event-1@google.com";
    let mut remote = FullListingRemote {
        items: vec![
            (
                String::from("event-1%40google.com.ics"),
                String::from(uid),
                b"BEGIN:VCALENDAR:one".to_vec(),
            ),
            (
                String::from("event-1%2540google.com.ics"),
                String::from(uid),
                b"BEGIN:VCALENDAR:two".to_vec(),
            ),
        ],
        fetched: Vec::new(),
        bound: HashMap::new(),
    };

    let first = spine(&mut store, &mut remote, "caldav/agenda");
    assert_eq!(
        first.item_patch.len(),
        2,
        "both resources are bodies to fetch on the first run",
    );
    assert_eq!(remote.fetched.len(), 2, "each body is downloaded once");

    let mut placements = load_side(&store, "caldav/agenda").unwrap();
    placements.sort_by(|a, b| a.handle.0.cmp(&b.handle.0));
    assert_eq!(placements.len(), 2, "the twin has a row of its own");
    assert!(
        placements.iter().all(|p| p.object.is_some()),
        "each copy holds its own body",
    );
    assert_ne!(
        placements[0].object, placements[1].object,
        "two resources, two bodies",
    );

    let keys: Vec<String> = placements
        .iter()
        .map(|p| p.link_id.clone().unwrap().0)
        .collect();
    let bare = keys
        .iter()
        .position(|key| key == uid)
        .expect("one copy keeps the identity bare");
    let minted = 1 - bare;
    assert_eq!(
        keys[minted],
        format!("dup:{uid}#{}", placements[minted].handle.0),
        "the other is minted on the href it came from",
    );

    let second = spine(&mut store, &mut remote, "caldav/agenda");
    assert!(
        second.item_patch.is_empty(),
        "a settled collection reports nothing: {:?}",
        second
            .item_patch
            .iter()
            .map(|e| e.hunk.to_string())
            .collect::<Vec<_>>(),
    );
    assert!(second.duplicates.is_empty());
    assert_eq!(
        remote.fetched.len(),
        2,
        "no body is downloaded twice, so no blob is orphaned",
    );
}

/// What a create delivers by names the hunk.
///
/// A frontend's compose is an add, a staged copy is a copy from its
/// origin, and a body the other side holds is a copy across.
#[test]
fn a_create_is_named_by_what_delivers_it() {
    let created = |origin: Option<PimdirOrigin>| PimdirPlacement {
        collection: PimdirCollectionId("imap/INBOX".into()),
        handle: PimdirHandle("\u{1}mid:q@x".into()),
        link_id: Some(PimdirLinkId("mid:q@x".into())),
        object: None,
        level: PimdirLevel::Full,
        summary: None,
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::default(),
        status: PimdirStatus::Created,
        conflict_revision: None,
        conflict_object: None,
        base: None,
        origin,
    };

    let add = placement_hunks("imap", "imap", "imap", "INBOX", &created(None));
    assert_eq!(add[0].to_string(), "add item mid:q@x in INBOX on imap");

    let origin = PimdirOrigin {
        collection: PimdirCollectionId("imap/Sent".into()),
        handle: PimdirHandle("7".into()),
    };
    let copy = placement_hunks("imap", "imap", "imap", "INBOX", &created(Some(origin)));
    assert_eq!(
        copy[0].to_string(),
        "copy item mid:q@x from Sent to INBOX on imap"
    );

    let across = placement_hunks("left", "right", "left", "INBOX", &created(None));
    assert_eq!(
        across[0].to_string(),
        "copy item mid:q@x in INBOX from right to left"
    );
}

/// A fallback that must never be reached: every body comes from its batch.
struct NoWire;

impl PimdirRemote for NoWire {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        anyhow::bail!("no wire")
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        _handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        anyhow::bail!("no wire")
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        _changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        anyhow::bail!("no wire")
    }
}

/// A message probed at `Meta`: its envelope, no body yet.
fn probed(collection: &str, handle: &str) -> PimdirWriteOp {
    PimdirWriteOp::UpsertPlacement(PimdirPlacement {
        collection: PimdirCollectionId(collection.into()),
        handle: PimdirHandle(handle.into()),
        link_id: Some(PimdirLinkId(format!("mid:{handle}@example.org"))),
        object: None,
        level: PimdirLevel::Meta,
        summary: None,
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::default(),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        base: None,
        origin: None,
    })
}

/// A run stopped while downloading keeps every batch it applied.
///
/// The bodies of an account used to be raised to `Full` only once all of
/// them had downloaded, so a run killed an hour in kept nothing and the
/// next one started over. Each batch is now applied as it lands.
#[test]
fn an_interrupted_hydration_keeps_the_batches_it_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = PimdirStore::open(dir.path())
        .unwrap()
        .for_account("mail")
        .for_source("imap");
    let handles = ["1", "2", "3", "4", "5", "6"];
    for collection in ["imap/INBOX", "imap/Archive"] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
        store
            .write(handles.iter().map(|h| probed(collection, h)).collect())
            .unwrap();
    }

    let batches = vec![
        batch("imap/Archive", &["1", "2"]),
        batch("imap/INBOX", &["1", "2", "3"]),
        batch("imap/Archive", &["3", "4"]),
        batch("imap/INBOX", &["4", "5", "6"]),
    ];

    let fetched = AtomicUsize::new(0);
    let mut workers = [&mut store];
    let outcome = hydrate_pool(&mut workers, batches, |store, batch| {
        if fetched.fetch_add(1, Ordering::Relaxed) == 2 {
            anyhow::bail!("connection lost");
        }
        let collection = &batch.collection;
        let items = batch
            .bodies
            .iter()
            .map(|(handle, _)| {
                let body = format!(
                    "Message-ID: <{}@example.org>\r\nSubject: {collection}\r\n\r\nhi\r\n",
                    handle.0
                );
                let blobs = PimdirStore::open(dir.path()).unwrap().blob_stage();
                PimdirFetchedItem {
                    handle: handle.clone(),
                    link_id: PimdirLinkId(format!("mid:{}@example.org", handle.0)),
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    body: Some(PimdirFetchedBody::Inline {
                        hash: blobs.blobs().hash(body.as_bytes()),
                        bytes: body.into_bytes(),
                    }),
                    revision: None,
                }
            })
            .collect();
        if let Err(err) = apply_batch(
            store,
            NoWire,
            collection,
            items,
            PimdirUpgradeOptions::default(),
        ) {
            warn!("{collection} write error: {err:#}");
        }
        Ok(())
    });

    assert_eq!(
        outcome.unwrap_err().to_string(),
        "connection lost",
        "the run still fails",
    );
    let kept = |collection: &str| -> Vec<String> {
        projection_view(&store, collection, "imap")
            .unwrap()
            .into_iter()
            .filter(|p| p.object.is_some() && p.level == PimdirLevel::Full)
            .map(|p| p.handle.0)
            .collect()
    };
    assert_eq!(kept("imap/Archive"), ["1", "2"]);
    assert_eq!(kept("imap/INBOX"), ["1", "2", "3"]);
    assert_eq!(fetched.load(Ordering::Relaxed), 3, "the pool stopped");
}

fn target(handle: &str, size: u64, date: Option<&str>) -> HydrateTarget {
    HydrateTarget {
        handle: PimdirHandle(handle.into()),
        size,
        date: date.map(String::from),
    }
}

fn queued(batches: Vec<HydrateBatch>) -> Vec<(String, Vec<String>)> {
    batches
        .into_iter()
        .map(|batch| {
            let handles = batch.bodies.into_iter().map(|(h, _)| h.0).collect();
            (batch.collection, handles)
        })
        .collect()
}

/// A batch of `handles` read from `collection`, of no known size.
fn batch(collection: &str, handles: &[&str]) -> HydrateBatch {
    HydrateBatch {
        collection: collection.to_string(),
        bodies: handles
            .iter()
            .map(|handle| (PimdirHandle((*handle).into()), 0))
            .collect(),
    }
}

/// The download order is the caller's: largest first by default, or
/// newest first.
///
/// Largest first is the fastest whole run; newest first keeps today's
/// mail from waiting behind years of large attachments.
#[test]
fn bodies_download_largest_or_newest_first() {
    let plans: Vec<CollectionPlan> = vec![
        (
            "imap/Archive".into(),
            vec![
                target("a", 900, Some("2019-05-01T08:00:00Z")),
                target("b", 10, Some("2026-10-03T08:00:00Z")),
            ],
        ),
        (
            "imap/INBOX".into(),
            vec![
                target("c", 50, Some("2026-10-04T08:00:00Z")),
                target("d", 20, None),
                target("e", 500, Some("2025-01-01T08:00:00Z")),
            ],
        ),
    ];
    let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();

    assert_eq!(
        queued(hydrate_batches(
            plans.clone(),
            DownloadOrder::Largest,
            0,
            false
        )),
        [
            ("imap/Archive".to_string(), owned(&["a", "b"])),
            ("imap/INBOX".to_string(), owned(&["e", "c", "d"])),
        ],
    );
    assert_eq!(
        queued(hydrate_batches(
            plans.clone(),
            DownloadOrder::Newest,
            0,
            false
        )),
        [
            ("imap/INBOX".to_string(), owned(&["c", "e", "d"])),
            ("imap/Archive".to_string(), owned(&["b", "a"])),
        ],
        "newest first, a mail without a date last",
    );
}

/// Newest first holds across collections batch by batch: an older batch
/// of the Inbox waits behind a newer one of another folder.
#[test]
fn newest_first_interleaves_collections_by_batch() {
    let inbox = (0..BATCH_SIZE + 1)
        .map(|i| {
            let date = format!("2026-10-04T{:02}:{:02}:00Z", i / 60, i % 60);
            target(&i.to_string(), 1, Some(&date))
        })
        .collect();
    let plans: Vec<CollectionPlan> = vec![
        ("imap/INBOX".into(), inbox),
        (
            "imap/Archive".into(),
            vec![target("x", 1, Some("2026-10-04T00:30:00Z"))],
        ),
    ];

    let order: Vec<(String, usize, String)> =
        hydrate_batches(plans.clone(), DownloadOrder::Newest, 0, false)
            .into_iter()
            .map(|batch| {
                let first = batch.bodies[0].0.0.clone();
                (batch.collection, batch.bodies.len(), first)
            })
            .collect();
    assert_eq!(
        order,
        [
            ("imap/INBOX".to_string(), BATCH_SIZE, BATCH_SIZE.to_string()),
            ("imap/Archive".to_string(), 1, "x".to_string()),
            ("imap/INBOX".to_string(), 1, "0".to_string()),
        ],
    );
}

/// A mail source answering rounds in scripted pages, newest first, its
/// cursor the index of the next page.
///
/// A band round carries no checkpoint, the store's staying the source's.
struct PagedRemote {
    /// Each page's members, as `(handle, Date header)`.
    pages: Vec<Vec<(&'static str, Option<&'static str>)>>,
    /// The page whose request fails once, as a dropped connection would.
    fail_at: Option<usize>,
    /// Whether the next resumed request is refused its cursor.
    reject_cursor: bool,
    /// Every request, in order.
    requests: Vec<PimdirEnumerate>,
}

impl PagedRemote {
    fn new(pages: Vec<Vec<(&'static str, Option<&'static str>)>>) -> Self {
        Self {
            pages,
            fail_at: None,
            reject_cursor: false,
            requests: Vec::new(),
        }
    }

    fn cursors(&self) -> Vec<Option<String>> {
        self.requests
            .iter()
            .map(|request| {
                request
                    .cursor()
                    .map(|cursor| String::from_utf8_lossy(&cursor.0).into_owned())
            })
            .collect()
    }
}

impl PimdirRemote for PagedRemote {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        self.requests.push(request.clone());
        let cursor = match &request.listing {
            PimdirListing::Head(_) => return Ok(PimdirEnumerated::NoHead),
            PimdirListing::Delta(checkpoint) => {
                return Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::delta(
                    Vec::new(),
                    Vec::new(),
                    checkpoint.clone(),
                )));
            }
            PimdirListing::Round { cursor, band } => (cursor.clone(), *band),
        };
        let (cursor, band) = cursor;
        if cursor.is_some() && mem::take(&mut self.reject_cursor) {
            return Ok(PimdirEnumerated::CursorRejected);
        }

        let index: usize = cursor
            .map(|cursor| String::from_utf8_lossy(&cursor.0).parse().unwrap())
            .unwrap_or(0);
        if self.fail_at == Some(index) {
            self.fail_at = None;
            anyhow::bail!("connection reset by peer");
        }

        let items = self.pages[index]
            .iter()
            .map(|(handle, date)| {
                let mut header = format!("Message-ID: <{handle}@example.org>\r\n");
                if let Some(date) = date {
                    header.push_str(&format!("Date: {date}\r\n"));
                }
                PimdirRemoteItem {
                    handle: PimdirHandle((*handle).into()),
                    flags: PimdirFlags::default(),
                    revision: None,
                    meta: io_pimdir::summary::mail::derive_meta(header.as_bytes(), None, None)
                        .into(),
                }
            })
            .collect();
        let next = (index + 1 < self.pages.len())
            .then(|| PimdirCursor((index + 1).to_string().into_bytes()));
        let checkpoint = (index == 0 && !band).then(|| PimdirCheckpoint(b"modseq-1".to_vec()));

        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::page(
            items, next, checkpoint,
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        _handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        anyhow::bail!("every member arrives named")
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        anyhow::bail!("nothing is staged, so nothing pushes: {changes:?}")
    }
}

/// A mailbox holding one message from 2020, synced before any scope.
fn mailbox_with_an_old_message(dir: &Path) -> PimdirSourceStore {
    let old = io_pimdir::summary::mail::derive_meta(
        b"Message-ID: <old@example.org>\r\nDate: Wed, 01 Jan 2020 10:00:00 +0000\r\n\r\n",
        None,
        None,
    );
    let mut store = PimdirStore::open(dir).unwrap().for_source("imap");
    store
        .ensure_collection("imap/INBOX", "message/rfc822")
        .unwrap();
    store
        .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId("imap/INBOX".into()),
            handle: PimdirHandle("1".into()),
            link_id: Some(old.link_id),
            object: None,
            level: PimdirLevel::Meta,
            summary: old.summary,
            sort_key: old.sort_key,
            flags: PimdirFlags::default(),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: Some(PimdirBase {
                flags: PimdirFlags::default(),
                revision: None,
                object: None,
            }),
            origin: None,
        })])
        .unwrap();
    store
}

fn scoped_sync(store: &mut PimdirSourceStore, remote: &mut PagedRemote) -> Result<()> {
    let verb = sync_verb(
        "imap/INBOX",
        true,
        PimdirPushRights::all(),
        PimdirConflictPolicy::Manual,
        PimdirScope::since("2026-09-07T00:00:00Z"),
        false,
    );
    run_verb(store, remote, verb).map(|_| ())
}

fn handles_of(store: &PimdirSourceStore) -> BTreeSet<String> {
    load_side(store, "imap/INBOX")
        .unwrap()
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect()
}

fn pages() -> Vec<Vec<(&'static str, Option<&'static str>)>> {
    vec![
        vec![
            ("31", Some("Tue, 06 Oct 2026 10:00:00 +0000")),
            ("30", Some("Tue, 01 Jan 2030 10:00:00 +0000")),
        ],
        vec![
            ("20", Some("Mon, 05 Oct 2026 10:00:00 +0000")),
            ("19", None),
        ],
        vec![("10", Some("Fri, 02 Oct 2026 10:00:00 +0000"))],
    ]
}

/// A round broken between two pages keeps what landed and resumes from
/// its cursor.
///
/// Its last page infers no delete out of scope, and nothing pushes a
/// delete nobody staged.
#[test]
fn an_interrupted_round_resumes_from_its_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    remote.fail_at = Some(1);

    let err = scoped_sync(&mut store, &mut remote).unwrap_err();
    assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
    assert_eq!(
        handles_of(&store),
        names(&["1", "30", "31"]),
        "page one landed"
    );
    let coverage = store.list_coverage("imap/INBOX").unwrap();
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].coverage.is_none(), "no round closed yet");
    assert!(coverage[0].round.is_some(), "the round is left open");

    scoped_sync(&mut store, &mut remote).unwrap();
    assert_eq!(
        remote.cursors(),
        [
            None,
            Some(String::from("1")),
            Some(String::from("1")),
            Some(String::from("2"))
        ],
        "the second run asked for the page the first never got",
    );
    assert_eq!(
        handles_of(&store),
        names(&["1", "10", "19", "20", "30", "31"]),
        "the 2020 message no page listed lies out of scope and stays",
    );
    let coverage = store.list_coverage("imap/INBOX").unwrap();
    let closed = coverage[0].coverage.as_ref().expect("the round closed");
    assert_eq!(closed.scope, PimdirScope::since("2026-09-07T00:00:00Z"));
    assert!(coverage[0].round.is_none());

    let requests = remote.requests.len();
    scoped_sync(&mut store, &mut remote).unwrap();
    assert!(
        matches!(remote.requests[requests].listing, PimdirListing::Delta(_)),
        "a covered scope lists the delta from the round's checkpoint",
    );
}

/// A cursor the source refuses restarts the round from its first page,
/// the members already landed matched by handle and never twice.
#[test]
fn a_refused_cursor_restarts_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    remote.fail_at = Some(1);
    scoped_sync(&mut store, &mut remote).unwrap_err();

    remote.reject_cursor = true;
    scoped_sync(&mut store, &mut remote).unwrap();
    assert_eq!(
        remote.cursors(),
        [
            None,
            Some(String::from("1")),
            Some(String::from("1")),
            None,
            Some(String::from("1")),
            Some(String::from("2")),
        ],
    );
    assert_eq!(
        handles_of(&store),
        names(&["1", "10", "19", "20", "30", "31"])
    );
}

/// A band round infers no delete of mail its date filter cannot list.
///
/// IMAP's `SENTSINCE` misses a garbled `Date`, Gmail's `after:` a message
/// with no `Date` received outside the band (io-pimdir `ff28408`).
#[test]
fn a_band_round_keeps_the_undated_mail_it_does_not_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(vec![
        vec![
            ("31", Some("Tue, 06 Oct 2026 10:00:00 +0000")),
            ("20", Some("Mon, 05 Oct 2026 10:00:00 +0000")),
        ],
        vec![("19", None), ("18", Some("sometime last week"))],
    ]);
    scoped_sync(&mut store, &mut remote).unwrap();
    assert_eq!(handles_of(&store), names(&["1", "18", "19", "20", "31"]));

    remote.pages = vec![vec![("5", Some("Mon, 02 Mar 2026 10:00:00 +0000"))]];
    let requests = remote.requests.len();
    let verb = sync_verb(
        "imap/INBOX",
        true,
        PimdirPushRights::all(),
        PimdirConflictPolicy::Manual,
        PimdirScope::since("2026-01-01T00:00:00Z"),
        false,
    );
    run_verb(&mut store, &mut remote, verb).unwrap();

    assert!(
        remote.requests[requests..]
            .iter()
            .all(|request| matches!(request.listing, PimdirListing::Round { band: true, .. })),
        "the widening lists its band alone: {:?}",
        &remote.requests[requests..],
    );
    assert_eq!(
        handles_of(&store),
        names(&["1", "5", "18", "19", "20", "31"]),
        "the band brings its message, the undated and garbled ones stay",
    );
    let coverage = store.list_coverage("imap/INBOX").unwrap();
    let closed = coverage[0].coverage.as_ref().expect("the band closed");
    assert_eq!(closed.scope, PimdirScope::since("2026-01-01T00:00:00Z"));
}

/// A pager that hydrates each landed page where the sync stands at rest,
/// as phase 1's idle connections do, recording what it was told.
#[derive(Default)]
struct HydratingPager {
    /// Each call, its landed handles sorted, and whether it resumed.
    told: Vec<(Vec<String>, bool)>,
    /// Whether a landed page gets its bodies.
    hydrate: bool,
    /// Listing waits begun, then ended.
    listing: usize,
    listed: usize,
}

impl Pager for HydratingPager {
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
        _next: Option<&PimdirListing>,
    ) -> Rest {
        assert_eq!(self.listing, self.listed, "never told mid-listing");
        let mut names: Vec<String> = landed.iter().map(|h| h.0.clone()).collect();
        names.sort();
        self.told.push((names, resumed));
        if landed.is_empty() {
            return Rest::Go;
        }

        let placements = store
            .load(collection, &PimdirLoadSelection::Handles(landed.clone()))
            .unwrap()
            .placements;
        assert_eq!(placements.len(), landed.len(), "the page has landed");
        if !self.hydrate {
            return Rest::Go;
        }
        let blobs = store.blob_stage();
        let items = placements
            .into_iter()
            .map(|placement| {
                let body = format!(
                    "Message-ID: <{}@example.org>\r\n\r\nhi\r\n",
                    placement.handle.0
                );
                PimdirFetchedItem {
                    handle: placement.handle,
                    link_id: placement.link_id.unwrap(),
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    body: Some(PimdirFetchedBody::Inline {
                        hash: blobs.blobs().hash(body.as_bytes()),
                        bytes: body.into_bytes(),
                    }),
                    revision: None,
                }
            })
            .collect();
        apply_batch(
            store,
            NoWire,
            collection.as_str(),
            items,
            PimdirUpgradeOptions::default(),
        )
        .unwrap();
        Rest::Go
    }

    fn listing(&mut self) {
        self.listing += 1;
    }

    fn listed(&mut self) {
        self.listed += 1;
    }
}

fn paged_sync(
    store: &mut PimdirSourceStore,
    remote: &mut PagedRemote,
    pager: &mut dyn Pager,
) -> Result<()> {
    let verb = sync_verb(
        "imap/INBOX",
        false,
        PimdirPushRights::all(),
        PimdirConflictPolicy::Manual,
        PimdirScope::since("2026-09-07T00:00:00Z"),
        false,
    );
    run_verb_paged(store, remote, verb, Some(pager)).map(|_| ())
}

fn bodied(store: &PimdirSourceStore) -> BTreeSet<String> {
    load_side(store, "imap/INBOX")
        .unwrap()
        .into_iter()
        .filter(|placement| placement.object.is_some())
        .map(|placement| placement.handle.0)
        .collect()
}

/// Each page is told once written, the last one when the sync completes.
///
/// A body applied there survives the pages that land after it, the last
/// one's whole-collection merge included.
#[test]
fn a_landed_page_is_told_at_rest_and_its_bodies_stay() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    let mut pager = HydratingPager {
        hydrate: true,
        ..Default::default()
    };

    paged_sync(&mut store, &mut remote, &mut pager).unwrap();

    let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();
    assert_eq!(
        pager.told,
        [
            (Vec::new(), false),
            (owned(&["30", "31"]), false),
            (owned(&["19", "20"]), false),
            (owned(&["10"]), false),
        ],
    );
    assert_eq!((pager.listing, pager.listed), (3, 3));
    assert_eq!(
        bodied(&store),
        names(&["10", "19", "20", "30", "31"]),
        "every page kept the bodies applied at rest",
    );
}

/// A round resumed is told so before its first page.
///
/// Its page feed then queues the bodies the earlier run's pages still
/// owe, newest first across the account, beside each page landed after.
#[test]
fn a_resumed_round_feeds_what_its_earlier_pages_owe() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    remote.fail_at = Some(1);

    let mut pager = HydratingPager::default();
    paged_sync(&mut store, &mut remote, &mut pager).unwrap_err();
    assert_eq!(pager.told.len(), 2, "page one was told before the failure");
    assert!(bodied(&store).is_empty());

    let mut pager = HydratingPager::default();
    paged_sync(&mut store, &mut remote, &mut pager).unwrap();
    assert_eq!(pager.told[0], (Vec::new(), true), "told once, resumed");
    assert!(pager.told[1..].iter().all(|(_, resumed)| !resumed));

    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    remote.fail_at = Some(1);
    paged_sync(&mut store, &mut remote, &mut HydratingPager::default()).unwrap_err();

    let gate = Mutex::new(());
    let work = Work::new(DownloadOrder::Newest, 0, false);
    let mut pager = PageFeed {
        feed: Some(&work),
        ..PageFeed::hold(&gate)
    };
    paged_sync(&mut store, &mut remote, &mut pager).unwrap();
    drop(pager);

    let mut fed = Vec::new();
    while let Some(job) = work.next() {
        let Job::Hydrate {
            batch,
            primed: false,
        } = job
        else {
            panic!("only page bodies are queued: {job:?}");
        };
        assert_eq!(batch.collection, "imap/INBOX");
        fed.push(
            batch
                .bodies
                .into_iter()
                .map(|(h, _)| h.0)
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        fed,
        [vec!["30", "31", "1"], vec!["20", "19"], vec!["10"]],
        "the resumed round's earlier page first, the 2020 mail it owes too",
    );
}

/// A connection with no collection left waits on the feed until a page
/// lands or every spine has ended.
#[test]
fn the_feed_waits_while_a_spine_may_still_feed() {
    let work = Work::new(DownloadOrder::Largest, 1, false);
    assert_eq!(
        work.next(),
        Some(Job::Spine {
            index: 0,
            pass: Pass::Whole
        })
    );
    thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            let mut got = Vec::new();
            while let Some(Job::Hydrate { batch, .. }) = work.next() {
                got.push(batch.bodies.len());
            }
            got
        });
        thread::sleep(std::time::Duration::from_millis(20));
        work.feed(
            "imap/INBOX",
            0,
            vec![target("a", 1, None), target("b", 9, None)],
            false,
        );
        thread::sleep(std::time::Duration::from_millis(20));
        work.spined(Pass::Whole, 0, false);
        assert_eq!(waiter.join().unwrap(), [2]);
    });
}

/// Runs a scoped sync of the INBOX under `pager`, answering whether it
/// paused.
fn paused_sync(
    store: &mut PimdirSourceStore,
    remote: &mut PagedRemote,
    pager: &mut dyn Pager,
) -> bool {
    let verb = sync_verb(
        "imap/INBOX",
        false,
        PimdirPushRights::all(),
        PimdirConflictPolicy::Manual,
        PimdirScope::since("2026-09-07T00:00:00Z"),
        false,
    );
    match run_verb_paged(store, remote, verb, Some(pager)).unwrap() {
        Paged::Done(_) => false,
        Paged::Paused(_) => true,
    }
}

/// A paused round leaves the store as a run killed after its first page
/// does: the page landed, the round open at its cursor.
///
/// A resumed round pauses before it asks anything and the next sync
/// resumes from the cursor and closes it; a delta is never paused.
#[test]
fn a_paused_round_stands_where_a_killed_one_would() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = mailbox_with_an_old_message(dir.path());
    let mut remote = PagedRemote::new(pages());
    let gate = Mutex::new(());
    let pausing = |gate| PageFeed {
        pause: true,
        ..PageFeed::hold(gate)
    };

    assert!(paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
    assert_eq!(remote.cursors(), [None], "the first page alone");
    assert_eq!(handles_of(&store), names(&["1", "30", "31"]));
    let coverage = store.list_coverage("imap/INBOX").unwrap();
    assert!(coverage[0].coverage.is_none(), "no round closed yet");
    assert!(coverage[0].round.is_some(), "the round is left open");

    assert!(paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
    assert_eq!(remote.cursors(), [None], "a resumed round asks nothing");

    assert!(!paused_sync(
        &mut store,
        &mut remote,
        &mut PageFeed::hold(&gate)
    ));
    assert_eq!(
        remote.cursors(),
        [None, Some(String::from("1")), Some(String::from("2"))],
        "the fill resumes from the cursor the first page left",
    );
    assert_eq!(
        handles_of(&store),
        names(&["1", "10", "19", "20", "30", "31"])
    );
    let coverage = store.list_coverage("imap/INBOX").unwrap();
    assert!(coverage[0].coverage.is_some(), "the round closed");
    assert!(coverage[0].round.is_none());

    let requests = remote.requests.len();
    assert!(!paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
    assert!(
        matches!(remote.requests[requests].listing, PimdirListing::Delta(_)),
        "a whole collection runs its delta to the end",
    );
}

/// Takes the next job, failing on none.
fn job(work: &Work) -> Job {
    work.next().expect("a job")
}

fn spine_job(index: usize, pass: Pass) -> Job {
    Job::Spine { index, pass }
}

/// Priming lists every first page, each followed by its newest bodies,
/// before any second page.
///
/// Pages go in the run's order; the fill waits for the last primed body
/// too, other bodies downloading meanwhile.
#[test]
fn every_first_page_and_its_newest_bodies_come_before_any_second_page() {
    let work = Work::new(DownloadOrder::Newest, 3, true);
    let primed = |collection: &str, handle: &str| Job::Hydrate {
        batch: HydrateBatch {
            collection: collection.to_string(),
            bodies: vec![(PimdirHandle(handle.into()), 1)],
        },
        primed: true,
    };

    assert_eq!(job(&work), spine_job(0, Pass::First));
    work.feed("INBOX", 0, vec![target("i1", 1, None)], true);
    work.spined(Pass::First, 0, true);
    assert_eq!(job(&work), primed("INBOX", "i1"), "its bodies right after");
    work.downloaded(true);

    assert_eq!(job(&work), spine_job(1, Pass::First));
    work.spined(Pass::First, 1, false);
    assert_eq!(job(&work), spine_job(2, Pass::First));
    work.feed("Trash", 2, vec![target("t1", 1, None)], true);
    work.feed("Sent", 1, vec![target("s1", 1, None)], false);
    work.spined(Pass::First, 2, true);
    assert_eq!(job(&work), primed("Trash", "t1"));

    assert!(
        matches!(job(&work), Job::Hydrate { primed: false, .. }),
        "a primed body under way holds the fill back, not the rest",
    );
    work.downloaded(false);
    work.downloaded(true);
    assert_eq!(job(&work), spine_job(0, Pass::Fill), "the fill, in order");
    assert_eq!(job(&work), spine_job(2, Pass::Fill));
    work.spined(Pass::Fill, 0, false);
    work.spined(Pass::Fill, 2, false);
    assert_eq!(work.next(), None);
}

/// Several connections take the first pass together, and no fill
/// starts while one of its spines is under way.
#[test]
fn the_fill_waits_for_every_first_page() {
    let work = Work::new(DownloadOrder::Newest, 2, true);
    assert_eq!(job(&work), spine_job(0, Pass::First));
    assert_eq!(job(&work), spine_job(1, Pass::First));
    work.spined(Pass::First, 0, true);
    thread::scope(|scope| {
        let filler = scope.spawn(|| job(&work));
        thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            !filler.is_finished(),
            "page 2 of one waits for page 1 of all"
        );
        work.spined(Pass::First, 1, false);
        assert_eq!(filler.join().unwrap(), spine_job(0, Pass::Fill));
    });
    work.spined(Pass::Fill, 0, false);
    assert_eq!(work.next(), None);
}

/// Queued bodies leave in the download order, a tie going to the
/// collection the run takes first, whichever was queued first.
#[test]
fn a_tie_between_batches_goes_to_the_collection_taken_first() {
    let work = Work::new(DownloadOrder::Newest, 0, false);
    let date = Some("2026-10-04T08:00:00Z");
    work.feed("Archive", 3, vec![target("a", 1, date)], false);
    work.feed("INBOX", 0, vec![target("i", 1, date)], false);
    work.feed(
        "Sent",
        1,
        vec![target("s", 1, Some("2026-10-05T08:00:00Z"))],
        false,
    );
    let mut order = Vec::new();
    while let Some(Job::Hydrate { batch, .. }) = work.next() {
        order.push(batch.collection);
    }
    assert_eq!(order, ["Sent", "INBOX", "Archive"]);
}

/// Phase 2 takes each collection's newest bodies ahead of the rest,
/// whatever the download order, then the rest in it.
#[test]
fn phase_two_downloads_the_newest_of_each_collection_first() {
    let plans: Vec<CollectionPlan> = vec![
        (
            "imap/INBOX".into(),
            vec![
                target("old", 900, Some("2019-05-01T08:00:00Z")),
                target("new", 10, Some("2026-10-03T08:00:00Z")),
            ],
        ),
        (
            "imap/Archive".into(),
            vec![
                target("big", 800, Some("2020-01-01T08:00:00Z")),
                target("recent", 5, Some("2026-10-01T08:00:00Z")),
            ],
        ),
    ];
    let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();
    assert_eq!(
        queued(hydrate_batches(
            plans.clone(),
            DownloadOrder::Largest,
            1,
            false
        )),
        [
            ("imap/INBOX".to_string(), owned(&["new"])),
            ("imap/Archive".to_string(), owned(&["recent"])),
            ("imap/INBOX".to_string(), owned(&["old"])),
            ("imap/Archive".to_string(), owned(&["big"])),
        ],
    );
}

/// The run takes the inbox first and the trash last, every collection
/// without a role in between by name.
#[test]
fn collections_are_taken_by_role_then_by_name() {
    let ids = [
        "Trash", "zeta", "INBOX", "Spam", "Archive", "Sent", "Drafts", "All", "beta",
    ];
    let kept: BTreeSet<String> = ids.iter().map(|id| id.to_string()).collect();
    let names: BTreeMap<String, String> = ids
        .iter()
        .map(|id| (id.to_string(), id.to_string()))
        .collect();
    let roles: BTreeMap<String, String> = [
        ("INBOX", "inbox"),
        ("Sent", "sent"),
        ("Drafts", "drafts"),
        ("Archive", "archive"),
        ("All", "all"),
        ("Spam", "junk"),
        ("Trash", "trash"),
    ]
    .iter()
    .map(|(id, role)| (id.to_string(), role.to_string()))
    .collect();

    assert_eq!(
        run_order(&kept, &names, &roles),
        [
            "INBOX", "Sent", "Drafts", "Archive", "beta", "zeta", "All", "Spam", "Trash"
        ],
    );
}

/// Under `--bodies listed`, only what a page of this run landed is
/// downloaded, the rest staying owed.
#[test]
fn listed_bodies_are_those_this_run_landed() {
    let listed = Listed::default();
    listed.record("imap/INBOX", &[PimdirHandle("2".into())]);
    let kept = listed.keep(
        "imap/INBOX",
        vec![target("1", 1, None), target("2", 1, None)],
    );
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].handle.0, "2");
    assert!(
        listed
            .keep("imap/Archive", vec![target("2", 1, None)])
            .is_empty()
    );
}

fn source_of(toml: &str, name: &str) -> SourceConfig {
    let config: AccountConfig = toml::from_str(toml).unwrap();
    config.sources().unwrap()[name].clone()
}

/// IMAP opens the run's connections, Graph three unless its pool size
/// says otherwise and never more than the run's, Google and DAV one.
#[test]
fn each_backend_opens_its_own_connection_budget() {
    let imap = source_of("imap.server = \"imaps://localhost\"", "imap");
    assert_eq!(connection_budget(&imap, 4), 4);
    assert_eq!(connection_budget(&imap, 8), 8);

    let graph = "msgraph.auth.token.raw = \"x\"\nmsgraph.mail = true";
    let msgraph = source_of(graph, "msgraph-mail");
    assert_eq!(connection_budget(&msgraph, 4), 3);
    assert_eq!(connection_budget(&msgraph, 1), 1, "--connections 1");
    let sized = source_of(
        "msgraph.auth.token.raw = \"x\"\nmsgraph.mail.pool-size = 2",
        "msgraph-mail",
    );
    assert_eq!(connection_budget(&sized, 4), 2);
    let calendar = source_of(
        "msgraph.auth.token.raw = \"x\"\nmsgraph.calendar = true",
        "msgraph-calendar",
    );
    assert_eq!(connection_budget(&calendar, 4), 3);

    let gmail = source_of("gmail.auth.token.raw = \"x\"", "gmail");
    assert_eq!(connection_budget(&gmail, 4), 1);
    let caldav = source_of(
        "caldav.server = \"https://localhost\"\ncaldav.auth.basic.username = \"u\"\ncaldav.auth.basic.password.raw = \"p\"",
        "caldav",
    );
    assert_eq!(connection_budget(&caldav, 4), 1);
}

/// Priming is on under the order a caller picks to show mail early,
/// off under the fastest whole run, and the caller may say otherwise.
#[test]
fn priming_defaults_by_the_download_order() {
    let wait = throttle::MAX_WAIT;
    let pacing = |order, prime| Pacing::new(order, prime, Bodies::All, wait).prime;
    assert_eq!(pacing(DownloadOrder::Newest, None), DEFAULT_PRIME);
    assert_eq!(pacing(DownloadOrder::Largest, None), 0);
    assert_eq!(pacing(DownloadOrder::Newest, Some(0)), 0);
    assert_eq!(pacing(DownloadOrder::Largest, Some(10)), 10);
}

/// A create the engine says its server rejected is reported once.
///
/// The connector's own record names it with its reason, else it reads as
/// a rejected `create`.
#[test]
fn a_rejected_create_is_named_once() {
    let report = PimdirSyncReport {
        rejected_creates: vec![
            PimdirHandle::from("a\u{1}1"),
            PimdirHandle::from("b\u{1}1"),
            PimdirHandle::from("c\u{1}1"),
        ],
        ..Default::default()
    };
    let rejected = [RejectedPush {
        collection: String::from("INBOX"),
        handle: String::from("a\u{1}1"),
        action: "append",
        failure: Failure::from("NO over quota"),
    }];
    let refused = [RefusedCreate {
        collection: String::from("INBOX"),
        uid: String::from("b"),
        handle: String::from("b\u{1}1"),
    }];

    let unnamed = unnamed_creates(&report, "INBOX", &rejected, &refused);

    let named: Vec<(&str, &str)> = unnamed
        .iter()
        .map(|push| (push.handle.as_str(), push.action))
        .collect();
    assert_eq!(named, [("c\u{1}1", "create")]);
}

/// A queued edit a policy settles against another body keeps the loser
/// as a retained item, which the run names by its row.
///
/// Nothing else reports a body kept aside while the queue was applied.
#[test]
fn a_body_the_drain_kept_aside_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let card =
        |name: &str| format!("BEGIN:VCARD\r\nVERSION:4.0\r\nUID:u1\r\nFN:{name}\r\nEND:VCARD\r\n");
    let bound = |handle: &str, body: &str| {
        let hash = io_pimdir::hash::hash(body.as_bytes());
        vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: hash.clone(),
                    size: body.len(),
                },
                body: Some(body.as_bytes().to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle(handle.into()),
                link_id: Some(PimdirLinkId("u1".into())),
                object: Some(hash.clone()),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some("1".into()),
                    object: Some(hash),
                }),
                origin: None,
            }),
        ]
    };

    let store = PimdirStore::open(dir.path()).unwrap();
    store.ensure_collection("contacts", "text/vcard").unwrap();
    store
        .set_collection_conflict("contacts", PimdirHubConflict::PreferIncoming)
        .unwrap();
    store
        .for_source("a")
        .write(bound("a1", &card("Ann")))
        .unwrap();
    let mut b = PimdirStore::open(dir.path()).unwrap().for_source("b");
    b.write(bound("b1", &card("Ann B"))).unwrap();
    let seq = b.list_items("contacts", None, 10).unwrap()[0].seq;
    drop(b);

    let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
    let row = producer
        .enqueue_with_body("contacts", card("Ann A").as_bytes(), |object| {
            PimdirAction::Update {
                seq,
                object,
                if_match: None,
                resolves: None,
            }
        })
        .unwrap();
    drop(producer);

    let mut owner = PimdirStore::open(dir.path()).unwrap().for_source("a");
    let mut report = SyncOutput::default();
    drain_queues(&mut owner, &mut report);

    let kept: Vec<String> = owner
        .list_retained("contacts", None, 10)
        .unwrap()
        .into_iter()
        .map(|item| item.link_id.0)
        .collect();
    assert_eq!(report.overruled.len(), 1, "{:?}", report.overruled);
    let overruled = &report.overruled[0];
    assert_eq!(overruled.action, Some(row.handle.id));
    assert_eq!(overruled.collection, "contacts");
    assert!(kept.contains(&overruled.kept), "{kept:?}");
}
