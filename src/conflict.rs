//! The divergences a run parked, and the decision that settles one.
//!
//! Every run three-way merges what nobody disagreed about (see
//! [`crate::kind::merge`]) and parks the rest. What is left is a genuine
//! disagreement: both sides changed the same field, and whose edit wins is
//! not a decision a sync can make. This module is the other end of that, the
//! vocabulary the conflict command reads the parked divergences with, plus
//! the one write that settles one.
//!
//! Deciding is a command and never a run. Nothing here is reached from a
//! sync, whatever is attached to its terminal: a run has one when a wrapper
//! script drives it, when a pane nobody is sitting at watches it and when a
//! person is waiting, and the three are indistinguishable from inside.
//!
//! A resolution is an ordinary edit. The chosen body is staged through the
//! store's queue as an update and drained in the same breath, which is
//! already the path whoever owns an edit resolves a conflict by, so a settled
//! body is written exactly one way. Nothing is pushed from here; the next run
//! does that, conditioned on the revision the divergence was recorded at.

pub mod merger;
pub mod report;

use std::{collections::HashMap, io::Write, path::Path};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use io_pimdir::{PimdirBlobs, PimdirProducer, PimdirReader, PimdirStore, codec::PimdirAction};
use io_replica::{object::ReplicaHash, placement::ReplicaLinkId};
use log::{info, warn};

use crate::kind::Kind;

/// One divergence waiting for a decision: an item whose local body and whose
/// remote body both moved away from the base the last sync agreed on.
#[derive(Clone, Debug)]
pub struct Conflict {
    /// The item's public id, which is what every neverest command addresses
    /// an item by. It is store-global and shared by the item's placements,
    /// so it names the card rather than one source's binding of it.
    pub id: i64,
    /// The store collection the item sits in, spelled `<namespace>/<name>`.
    pub collection: String,
    /// The IANA media type that collection is declared with, which is what
    /// picks the parse a settled body is summarized through and the
    /// extension an export is written under.
    pub media_type: String,
    /// The source whose own sync is stuck on the divergence. One source may
    /// be conflicted while another holding the same item is in sync, which
    /// is why a decision names this as well as the item.
    pub source: String,
    /// The item's handle on that source, which the next run pushes to.
    pub handle: String,
    /// The item's cross-source identity, which the same binding is found
    /// again by when a decision is applied.
    pub link_id: ReplicaLinkId,
    /// The remote revision observed when the divergence was recorded, or
    /// `None` from a remote reporting none. A decision computed against it
    /// is stale once it moves, which is what [`Conflict::apply`] refuses on.
    pub revision: Option<String>,
    /// The body the last sync agreed on, the merge's common ancestor.
    pub base: Option<ReplicaHash>,
    /// The local side of the divergence, the item's own body.
    pub local: Option<ReplicaHash>,
    /// The remote side at [`revision`](Self::revision), or `None` until a
    /// run's upgrade pass supplies it.
    pub remote: Option<ReplicaHash>,
}

/// What applying a decision concluded.
///
/// Only the first of the three changed anything, and the other two are
/// outcomes rather than failures: the store moved under a decision that took
/// a person some minutes to make, which is ordinary and is exactly what the
/// guard exists to notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Applied {
    /// The chosen body was staged as an edit and drained, so the item is no
    /// longer conflicted.
    Resolved,
    /// The store has observed a newer remote revision than the one the
    /// decision was computed against, so nothing was pushed. Carries what
    /// the store holds now.
    Moved(Option<String>),
    /// The divergence is gone: another run, or another decision, settled it
    /// while this one was being made.
    Settled,
}

/// The three bodies one divergence is between, read out of the store.
///
/// Each is absent for the same reason its hash was: a base a source never
/// recorded a body for, and a remote side a run has not fetched yet.
#[derive(Clone, Debug, Default)]
pub struct Sides {
    /// The body the last sync agreed on.
    pub base: Option<Vec<u8>>,
    /// The local side of the divergence.
    pub local: Option<Vec<u8>>,
    /// The remote side of the divergence.
    pub remote: Option<Vec<u8>>,
}

impl Conflict {
    /// Whether a decision can be made about this divergence at all.
    ///
    /// A conflict is marked with the diverging remote body wanted rather than
    /// held, the engine fetching nothing by itself, so one whose remote side
    /// has not landed yet is visible and listable and not resolvable. It
    /// becomes resolvable on the next run, which fetches it.
    pub fn resolvable(&self) -> bool {
        self.remote.is_some()
    }

    /// The kind the collection declares, or the error naming the media type
    /// this build cannot settle an item of.
    pub fn kind(&self) -> Result<Kind> {
        Kind::from_media_type(&self.media_type).with_context(|| {
            format!(
                "This build cannot settle items of type {} (collection {})",
                self.media_type, self.collection
            )
        })
    }

    /// Reads the three bodies out of the blob store.
    pub fn sides(&self, blobs: &PimdirBlobs) -> Result<Sides> {
        let read = |hash: &Option<ReplicaHash>| -> Result<Option<Vec<u8>>> {
            let Some(hash) = hash else {
                return Ok(None);
            };

            blobs
                .get(hash)
                .with_context(|| format!("Read the body {} of conflict {}", hash.as_str(), self.id))
        };

        Ok(Sides {
            base: read(&self.base)?,
            local: read(&self.local)?,
            remote: read(&self.remote)?,
        })
    }

    /// Applies `body` as the item's content, which settles the divergence.
    ///
    /// The staleness guard comes first. An unresolved conflict tracks the
    /// newest remote revision on every run, so a decision left in an editor
    /// for an hour can be a decision about a version nobody holds any more,
    /// and pushing it would overwrite everything that arrived meanwhile,
    /// which is the loss the parking exists to prevent arriving at the last
    /// step instead of the first. The store is therefore read again here,
    /// under the caller's lock, and a revision that moved is reported rather
    /// than applied.
    ///
    /// What follows is the edit path a run's own merge takes: the body into
    /// the blob tree, an `update` onto the queue, and the collection drained
    /// in the same breath.
    pub fn apply(&self, dir: &Path, account: &str, body: &[u8]) -> Result<Applied> {
        let mut store = PimdirStore::open(dir)
            .with_context(|| format!("Open the store of account {account}"))?
            .for_account(account)
            .for_source(&self.source);

        let observed = list(&store, account)?.into_iter().find(|observed| {
            observed.collection == self.collection
                && observed.link_id == self.link_id
                && observed.source == self.source
        });

        let Some(observed) = observed else {
            return Ok(Applied::Settled);
        };

        if observed.revision != self.revision {
            return Ok(Applied::Moved(observed.revision));
        }

        let kind = self.kind()?;

        // NOTE: before the blob write, so a body no parser reads never
        // reaches the tree at all. The automatic merge refuses the same
        // thing with `Merged::Unmergeable`; this is the half a person, or
        // the merger they named, writes by hand.
        kind.validate_body(body, &self.link_id)
            .with_context(|| format!("Settle conflict {} in {}", self.id, self.collection))?;

        let blobs = store.blobs();

        // NOTE: opened before the first blob write rather than at it, the
        // producer's staging lock being what keeps a collector out of the
        // window between a body reaching the blob tree and the queue row
        // pinning it.
        let mut producer = PimdirProducer::open(dir, env!("CARGO_PKG_NAME"))
            .with_context(|| format!("Stage the resolution of conflict {}", self.id))?;

        let hash = blobs.hash(body);
        let mut writer = blobs
            .writer()
            .with_context(|| format!("Store the settled body of conflict {}", self.id))?;
        writer
            .write_all(body)
            .with_context(|| format!("Store the settled body of conflict {}", self.id))?;
        let size = writer
            .commit(&hash)
            .with_context(|| format!("Store the settled body of conflict {}", self.id))?;

        let (_, meta, _) = kind.parse_body(body, size);

        producer
            .enqueue(
                &self.collection,
                &PimdirAction::Update {
                    seq: self.id,
                    object: hash,
                    meta: Some(meta),
                },
                Some(size),
                &Utc::now().to_rfc3339(),
            )
            .with_context(|| format!("Stage the settled body of conflict {}", self.id))?;

        drop(producer);

        let drained = store
            .drain_collection(&self.collection)
            .with_context(|| format!("Apply the settled conflict {}", self.id))?;

        if drained.parked > 0 {
            bail!(
                "The resolution of conflict {} could not be applied and parked",
                self.id
            );
        }

        if drained.applied == 0 {
            bail!("The resolution of conflict {} was not applied", self.id);
        }

        info!("resolved conflict {} in {}", self.id, self.collection);

        Ok(Applied::Resolved)
    }
}

/// The divergences an account's store is holding, by collection then item
/// then source.
///
/// The store answers this off a partial index over the conflicted flag, so a
/// store with nothing outstanding pays for an empty index rather than for a
/// pass over every collection.
pub fn list(store: &PimdirReader, account: &str) -> Result<Vec<Conflict>> {
    let parked = store
        .list_conflicts(Some(account))
        .with_context(|| format!("List the conflicts of account {account}"))?;

    let mut conflicts = Vec::with_capacity(parked.len());
    let mut media_types: HashMap<String, String> = HashMap::new();

    for conflict in parked {
        let seq = store
            .seq_for_link(&conflict.collection, &conflict.link_id.0)
            .with_context(|| {
                format!(
                    "Resolve the id of {} in {}",
                    conflict.handle.0, conflict.collection
                )
            })?;

        let Some(id) = seq else {
            warn!(
                "conflicted item {} in {} has no row of its own",
                conflict.handle.0, conflict.collection
            );
            continue;
        };

        let media_type = match media_types.get(&conflict.collection) {
            Some(media_type) => media_type.clone(),
            None => {
                let media_type = store
                    .collection_kind(&conflict.collection)
                    .with_context(|| {
                        format!("Read the kind of collection {}", conflict.collection)
                    })?
                    .unwrap_or_default();
                media_types.insert(conflict.collection.clone(), media_type.clone());
                media_type
            }
        };

        conflicts.push(Conflict {
            id,
            media_type,
            collection: conflict.collection,
            source: conflict.source.0,
            handle: conflict.handle.0,
            link_id: conflict.link_id,
            revision: conflict.conflict_revision,
            base: conflict.base_object,
            local: conflict.object,
            remote: conflict.conflict_object,
        });
    }

    Ok(conflicts)
}

/// The one conflict `id` names, narrowed by `source` when the item diverged
/// on more than one of them.
///
/// An id names an item and a divergence is one source's, so the two are not
/// the same arity. They coincide for every account with one source of a
/// kind, which is most of them, and the ambiguity is named rather than
/// guessed at everywhere else.
pub fn find(conflicts: Vec<Conflict>, id: i64, source: Option<&str>) -> Result<Conflict> {
    let mut found: Vec<Conflict> = conflicts
        .into_iter()
        .filter(|conflict| {
            conflict.id == id && source.is_none_or(|source| conflict.source == source)
        })
        .collect();

    if found.len() > 1 {
        let sources: Vec<&str> = found
            .iter()
            .map(|conflict| conflict.source.as_str())
            .collect();
        bail!(
            "Item {id} diverged on several sources ({}), name one with --source",
            sources.join(", ")
        );
    }

    match found.pop() {
        Some(conflict) => Ok(conflict),
        None => match source {
            Some(source) => bail!("Cannot find a conflict {id} on source {source}"),
            None => bail!("Cannot find a conflict {id}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use io_pimdir::PimdirSourceStore;
    use io_replica::{
        change::ReplicaWriteOp,
        client::ReplicaStorage,
        collection::ReplicaCollectionId,
        object::ReplicaObject,
        placement::{
            ReplicaBase, ReplicaFlags, ReplicaHandle, ReplicaLevel, ReplicaMeta, ReplicaPlacement,
            ReplicaSortKey, ReplicaStatus,
        },
    };

    use super::*;
    use crate::offline::storage::load_side;

    /// The account a conflicted store is grouped under.
    const ACCOUNT: &str = "cards";

    /// The revision the store recorded the divergence at.
    const REVISION: &str = "etag-2";

    /// The identity every seeded card states, and the one every placement
    /// here is linked by. The fixture states it rather than leaving the body
    /// and the link id to disagree, a settled body having to keep it.
    const UID: &str = "uid:a";

    /// A card carrying one phone number, which is the field the two sides of
    /// the seeded divergence set differently.
    fn card(tel: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{UID}\r\nFN:Jane Doe\r\nTEL:{tel}\r\nEND:VCARD\r\n"
        )
    }

    /// Seeds a store holding one card the engine marked conflicted, with all
    /// three bodies present, which is the state a decision is made from.
    fn store_with_conflict(dir: &Path) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir)
            .unwrap()
            .for_account(ACCOUNT)
            .for_source("dav");
        store.ensure_collection("contacts", "text/vcard").unwrap();

        let blobs = store.blobs();
        let stored = |body: String| ReplicaWriteOp::StoreObject {
            object: ReplicaObject {
                hash: blobs.hash(body.as_bytes()),
                size: body.len(),
            },
            body: Some(body.into_bytes()),
        };

        store
            .write(vec![
                stored(card("+1")),
                stored(card("+2")),
                stored(card("+3")),
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("contacts".into()),
                    handle: ReplicaHandle("card1".into()),
                    link_id: Some(ReplicaLinkId(UID.into())),
                    object: Some(blobs.hash(card("+2").as_bytes())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Conflict,
                    conflict_revision: Some(String::from(REVISION)),
                    conflict_object: Some(blobs.hash(card("+3").as_bytes())),
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(blobs.hash(card("+1").as_bytes())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        store
    }

    /// A decision computed against a revision the store has since moved past
    /// is a decision about a version nobody holds any more, and pushing it
    /// would overwrite whatever arrived meanwhile. It is reported as moved
    /// and changes nothing, and the same decision against the revision the
    /// store does hold settles the item, so the guard discriminates rather
    /// than refusing everything.
    #[test]
    fn a_resolution_against_a_moved_revision_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let blobs = store.blobs();
        let local = blobs.hash(card("+2").as_bytes());

        let conflicts = list(&store, ACCOUNT).unwrap();
        assert_eq!(conflicts.len(), 1);

        let conflict = find(conflicts.clone(), conflicts[0].id, Some("dav")).unwrap();
        assert_eq!(conflict.revision.as_deref(), Some(REVISION));

        let stale = Conflict {
            revision: Some(String::from("etag-1")),
            ..conflict.clone()
        };
        assert_eq!(
            stale
                .apply(dir.path(), ACCOUNT, card("+4").as_bytes())
                .unwrap(),
            Applied::Moved(Some(String::from(REVISION))),
        );

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_eq!(placement.status, ReplicaStatus::Conflict);
        assert_eq!(placement.object, Some(local), "nothing was pushed");

        assert_eq!(
            conflict
                .apply(dir.path(), ACCOUNT, card("+4").as_bytes())
                .unwrap(),
            Applied::Resolved,
        );

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_ne!(placement.status, ReplicaStatus::Conflict);
        let body = blobs.get(&placement.object.unwrap()).unwrap().unwrap();
        assert_eq!(String::from_utf8(body).unwrap(), card("+4"));
        assert!(list(&store, ACCOUNT).unwrap().is_empty());
    }

    /// A divergence whose remote body no run has fetched yet is a listing
    /// entry and not a decision. The engine marks a conflict with that body
    /// wanted rather than held, so this is the state every conflict passes
    /// through, and reading it as resolvable would hand `--prefer-remote` a
    /// side that is not there.
    #[test]
    fn a_conflict_waiting_for_its_diverging_body_is_listed_and_not_resolvable() {
        use crate::conflict::report::ConflictSummary;

        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let blobs = store.blobs();

        let conflicts = list(&store, ACCOUNT).unwrap();
        let fetched = find(conflicts.clone(), conflicts[0].id, None).unwrap();
        assert!(fetched.resolvable());

        let waiting = Conflict {
            remote: None,
            ..fetched
        };
        assert!(!waiting.resolvable());

        let sides = waiting.sides(&blobs).unwrap();
        assert!(sides.base.is_some());
        assert!(sides.local.is_some());
        assert!(
            sides.remote.is_none(),
            "a merger handed an absent remote side would merge against nothing"
        );

        let summary = ConflictSummary::from(&waiting);
        assert!(!summary.resolvable);
        let listed = summary.to_string();
        assert!(
            listed.contains("waiting for its diverging body"),
            "{listed}"
        );
    }

    /// A body no parser reads is not a decision, whoever wrote it.
    ///
    /// A tool that crashed after a partial write, or a person who saved a
    /// half-finished template, would otherwise replace a real card with
    /// something that is not one: the item keeps its link id and loses every
    /// field the identity was derived from. The automatic merge already
    /// refuses exactly this, with `Merged::Unmergeable`.
    #[test]
    fn a_settled_body_that_no_parser_reads_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let blobs = store.blobs();

        let conflicts = list(&store, ACCOUNT).unwrap();
        let conflict = find(conflicts.clone(), conflicts[0].id, None).unwrap();

        let err = conflict
            .apply(dir.path(), ACCOUNT, b"this is not a card at all")
            .unwrap_err();
        assert!(format!("{err:#}").contains("BEGIN:VCARD"), "{err:#}");

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_eq!(
            placement.status,
            ReplicaStatus::Conflict,
            "the refusal leaves the divergence exactly as it was",
        );
        assert_eq!(
            placement.object,
            Some(blobs.hash(card("+2").as_bytes())),
            "and leaves the local side untouched",
        );
    }

    /// A resolution that drops or changes the item's `UID` is a resolution of
    /// some other item.
    ///
    /// The bytes read as a card here, so nothing structural catches it: what
    /// does is that the store addresses the row by an identity its content no
    /// longer states, which is exactly the state a frontend reads as one
    /// contact and the server as another.
    #[test]
    fn a_settled_body_that_renames_the_item_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());

        let conflicts = list(&store, ACCOUNT).unwrap();
        let conflict = find(conflicts.clone(), conflicts[0].id, None).unwrap();

        let renamed = card("+4").replace(UID, "uid:someone-else");
        let err = conflict
            .apply(dir.path(), ACCOUNT, renamed.as_bytes())
            .unwrap_err();
        assert!(format!("{err:#}").contains("uid:someone-else"), "{err:#}");

        let dropped = card("+4").replace(&format!("UID:{UID}\r\n"), "");
        let err = conflict
            .apply(dir.path(), ACCOUNT, dropped.as_bytes())
            .unwrap_err();
        assert!(format!("{err:#}").contains("states none"), "{err:#}");
    }
}
