//! # Conflicts
//!
//! The divergences a run parked, and the decision that settles one. Every run
//! three-way merges what nobody disagreed about (see [`merge`])
//! and parks the genuine disagreements, whose winner a sync cannot pick.
//!
//! Deciding is a command and never a run: nothing here is reached from a
//! sync, whatever is attached to its terminal.
//!
//! A resolution is an ordinary edit, staged through the store's queue and
//! drained in the same breath, so a settled body is written exactly one way.
//! Nothing is pushed from here; the next run does that, conditioned on the
//! revision the divergence was recorded at.

pub mod merge;
pub mod merger;
pub mod report;

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail};
use io_pimdir::{
    client::{
        PimdirStore,
        blobs::PimdirBlobs,
        producer::{PimdirActionStatus, PimdirProducer},
        reader::PimdirReader,
    },
    codec::PimdirAction,
    object::PimdirHash,
    placement::PimdirLinkId,
};
use log::{info, warn};
use pimalaya_cli::error::CodedError;

use crate::{
    conflict::report::PartialCapability,
    error::{ErrorCode, pimdir_error},
    kind::Kind,
};

/// One divergence waiting for a decision: an item whose local body and whose
/// remote body both moved away from the base the last sync agreed on.
#[derive(Clone, Debug)]
pub struct Conflict {
    /// The item's public id, which every neverest command addresses it by.
    ///
    /// Store-global and shared by the item's placements, so it names the card
    /// rather than one source's binding of it.
    pub id: i64,
    /// The account whose collection holds the item.
    pub account: String,
    /// The store collection the item sits in, spelled
    /// `<account>/<namespace>/<name>`.
    pub collection: String,
    /// The IANA media type that collection is declared with.
    ///
    /// It picks the parse a settled body is summarized through, and the
    /// extension an export is written under.
    pub media_type: String,
    /// The source whose own sync is stuck on the divergence, by its pimdir
    /// source id (`<account>/<name>`), or `None` for the item's own conflict.
    ///
    /// One source may be conflicted while another holding the same item is in
    /// sync, which is why a decision names this as well as the item. Two
    /// endpoints disagreeing is the item's, no one source's (pimdir SYNC §9).
    pub source: Option<String>,
    /// The item's handle on that source, `None` for the item's own conflict.
    pub handle: Option<String>,
    /// The item's cross-source identity, which finds the same binding again
    /// when a decision is applied.
    pub link_id: PimdirLinkId,
    /// The remote revision observed when the divergence was recorded.
    ///
    /// `None` from a remote reporting none, and for the item's own conflict. A decision computed against it is
    /// stale once it moves, which is what [`Conflict::apply`] refuses on.
    pub revision: Option<String>,
    /// The body the last sync agreed on, the merge's common ancestor.
    ///
    /// For the item's own conflict, the shared body a binding last agreed on
    /// (see [`common_ancestor`]).
    pub base: Option<PimdirHash>,
    /// The local side of the divergence, the item's own body.
    pub local: Option<PimdirHash>,
    /// The remote side at [`revision`](Self::revision), the item's own
    /// diverging body, or `None` until a run's upgrade pass supplies it.
    pub remote: Option<PimdirHash>,
}

/// What applying a decision concluded.
///
/// Only the first of the three changed anything; the other two are outcomes
/// rather than failures, the store having moved under a decision that took a
/// person some minutes to make, which is what the guard exists to notice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Applied {
    /// The chosen body was staged as an edit and drained, the item settled.
    ///
    /// Carries the capabilities a source supports in part that the edit
    /// relies on.
    Resolved(Vec<PartialCapability>),
    /// The store has observed a newer remote revision than the decision was
    /// computed against, so nothing was pushed. Carries what it holds now.
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
    /// The source as the account's configuration names it, `None` for the
    /// item's own conflict.
    pub fn endpoint(&self) -> Option<&str> {
        let source = self.source.as_deref()?;
        let prefix = format!("{}/", self.account);
        Some(source.strip_prefix(&prefix).unwrap_or(source))
    }

    /// Whether a decision can be made about this divergence at all.
    ///
    /// A conflict is marked with the diverging remote body wanted rather than
    /// held, the engine fetching nothing by itself, so one whose remote side
    /// has not landed is listable and not resolvable until the next run.
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
        let read = |hash: &Option<PimdirHash>| -> Result<Option<Vec<u8>>> {
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
    /// A stale decision is refused: the store is read again under the caller's
    /// lock and a moved revision, or a moved side of the item's own conflict,
    /// reported, not pushed. A body no parser reads is refused.
    pub fn apply(&self, dir: &Path, account: &str, body: &[u8]) -> Result<Applied> {
        let store = PimdirStore::open(dir)
            .map_err(pimdir_error)
            .with_context(|| format!("Open the store of account {account}"))?
            .for_account(account);

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

        if self.source.is_none() && (observed.local != self.local || observed.remote != self.remote)
        {
            return Ok(Applied::Moved(None));
        }

        let source = match &self.source {
            Some(source) => source.clone(),
            None => store
                .item_bindings(&self.collection, &self.link_id.0)
                .map_err(pimdir_error)
                .with_context(|| format!("Read the bindings of conflict {}", self.id))?
                .into_keys()
                .next()
                .map(|source| source.0)
                .with_context(|| format!("Conflict {} has no binding to settle", self.id))?,
        };
        let mut store = store.for_source(source);

        let kind = self.kind()?;

        kind.validate_body(body, &self.link_id)
            .with_context(|| format!("Settle conflict {} in {}", self.id, self.collection))?;

        let mut producer = PimdirProducer::open(dir, env!("CARGO_PKG_NAME"))
            .map_err(pimdir_error)
            .with_context(|| format!("Stage the resolution of conflict {}", self.id))?;
        let enqueued = producer
            .enqueue_with_body(&self.collection, body, |object| PimdirAction::Update {
                seq: self.id,
                object,
                if_match: self.local.clone(),
                resolves: self.remote.clone().map(|remote| vec![remote]),
            })
            .map_err(pimdir_error)
            .with_context(|| format!("Stage the settled body of conflict {}", self.id))?;

        drop(producer);

        store
            .drain()
            .with_context(|| format!("Apply the settled conflict {}", self.id))?;

        let status = store
            .action_status(enqueued.handle.id)
            .map_err(pimdir_error)
            .with_context(|| format!("Read where the resolution of {} stands", self.id))?;
        match status {
            PimdirActionStatus::Applied { .. } => {}
            PimdirActionStatus::Parked { error, .. } => bail!(
                "The resolution of conflict {} could not be applied and parked: {error}",
                self.id
            ),
            _ => bail!("The resolution of conflict {} was not applied", self.id),
        }

        info!("resolved conflict {} in {}", self.id, self.collection);

        let partial = enqueued
            .partial
            .into_iter()
            .map(|partial| PartialCapability::new(partial, &self.account))
            .collect();
        Ok(Applied::Resolved(partial))
    }
}

/// The divergences the store holds, by collection then item then source.
///
/// A binding's conflicts come first, then the items' own (pimdir SYNC §9): two
/// endpoints disagreeing, each sorted by collection then item. The store
/// answers both off partial indexes over the conflicted flags.
pub fn list(store: &PimdirReader, account: &str) -> Result<Vec<Conflict>> {
    let parked = store
        .list_conflicts(Some(account))
        .with_context(|| format!("List the conflicts of account {account}"))?;
    let diverged = store
        .list_item_conflicts(Some(account))
        .with_context(|| format!("List the item conflicts of account {account}"))?;

    let mut conflicts = Vec::with_capacity(parked.len() + diverged.len());
    let mut media_types: HashMap<String, String> = HashMap::new();
    let mut media_type = |collection: &str| -> Result<String> {
        if let Some(media_type) = media_types.get(collection) {
            return Ok(media_type.clone());
        }
        let media_type = store
            .collection_kind(collection)
            .with_context(|| format!("Read the kind of collection {collection}"))?
            .unwrap_or_default();
        media_types.insert(collection.to_owned(), media_type.clone());
        Ok(media_type)
    };

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

        conflicts.push(Conflict {
            id,
            account: account.to_owned(),
            media_type: media_type(&conflict.collection)?,
            collection: conflict.collection,
            source: Some(conflict.source.0),
            handle: Some(conflict.handle.0),
            link_id: conflict.link_id,
            revision: conflict.conflict_revision,
            base: conflict.base_object,
            local: conflict.object,
            remote: conflict.conflict_object,
        });
    }

    for conflict in diverged {
        let base = common_ancestor(
            store,
            &conflict.collection,
            &conflict.link_id,
            conflict.object.as_ref(),
            conflict.conflict_object.as_ref(),
        )?;

        conflicts.push(Conflict {
            id: conflict.seq,
            account: account.to_owned(),
            media_type: media_type(&conflict.collection)?,
            collection: conflict.collection,
            source: None,
            handle: None,
            link_id: conflict.link_id,
            revision: None,
            base,
            local: conflict.object,
            remote: conflict.conflict_object,
        });
    }

    Ok(conflicts)
}

/// The body an item's own conflict diverged from, its merge's ancestor.
///
/// The shared body a binding last agreed on and neither side holds (pimdir
/// SYNC §9, `shared_object`, else its sync base), which the diverging binding
/// keeps pinned; `None` for two bodies its endpoints never agreed on.
pub fn common_ancestor(
    store: &PimdirReader,
    collection: &str,
    link_id: &PimdirLinkId,
    local: Option<&PimdirHash>,
    remote: Option<&PimdirHash>,
) -> Result<Option<PimdirHash>> {
    let bindings = store
        .item_bindings(collection, &link_id.0)
        .with_context(|| format!("Read the bindings of {} in {collection}", link_id.0))?;

    Ok(bindings
        .into_values()
        .filter_map(|binding| {
            let base = binding.base.and_then(|base| base.object);
            binding.shared_object.or(base)
        })
        .find(|agreed| Some(agreed) != local && Some(agreed) != remote))
}

/// The one conflict `id` names, narrowed by `source`, as the account's
/// configuration names it, when several diverged.
///
/// An id names an item and a divergence is one source's, so the two are not
/// the same arity. They coincide for every account with one source of a kind,
/// and the ambiguity is named rather than guessed at everywhere else.
pub fn find(conflicts: Vec<Conflict>, id: i64, source: Option<&str>) -> Result<Conflict> {
    let mut found: Vec<Conflict> = conflicts
        .into_iter()
        .filter(|conflict| {
            conflict.id == id && source.is_none_or(|source| conflict.endpoint() == Some(source))
        })
        .collect();

    if found.len() > 1 {
        let sources: Vec<&str> = found
            .iter()
            .map(|conflict| conflict.endpoint().unwrap_or("the item itself"))
            .collect();
        bail!(
            "Item {id} diverged on several sources ({}), name one with --source",
            sources.join(", ")
        );
    }

    if let Some(conflict) = found.pop() {
        return Ok(conflict);
    }

    let message = match source {
        Some(source) => format!("Cannot find a conflict {id} on source {source}"),
        None => format!("Cannot find a conflict {id}"),
    };
    Err(CodedError::new(ErrorCode::ConflictNotFound, message).into())
}

#[cfg(test)]
mod tests {
    use io_pimdir::{
        change::PimdirWriteOp,
        client::PimdirSourceStore,
        collection::PimdirCollectionId,
        object::PimdirObject,
        placement::{
            PimdirBase, PimdirFlags, PimdirHandle, PimdirLevel, PimdirPlacement, PimdirSortKey,
            PimdirStatus,
        },
    };

    use super::*;
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    use crate::sync::storage::load_side;

    /// The account a conflicted store is grouped under.
    const ACCOUNT: &str = "cards";

    /// The revision the store recorded the divergence at.
    const REVISION: &str = "etag-2";

    /// The identity every seeded card states and every placement is linked
    /// by, a settled body having to keep it.
    const UID: &str = "uid:a";

    /// A card carrying one phone number, the field the two sides of the
    /// seeded divergence set differently.
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
        let stored = |body: String| PimdirWriteOp::StoreObject {
            object: PimdirObject {
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
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId(UID.into())),
                    object: Some(blobs.hash(card("+2").as_bytes())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Conflict,
                    conflict_revision: Some(String::from(REVISION)),
                    conflict_object: Some(blobs.hash(card("+3").as_bytes())),
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(blobs.hash(card("+1").as_bytes())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        store
    }

    /// A decision against a revision the store moved past is reported as
    /// moved and changes nothing.
    ///
    /// It would overwrite whatever arrived meanwhile. The same decision
    /// against the revision held settles the item: the guard discriminates.
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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
        assert_eq!(placement.status, PimdirStatus::Conflict);
        assert_eq!(placement.object, Some(local), "nothing was pushed");

        assert_eq!(
            conflict
                .apply(dir.path(), ACCOUNT, card("+4").as_bytes())
                .unwrap(),
            Applied::Resolved(Vec::new()),
        );

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_ne!(placement.status, PimdirStatus::Conflict);
        let body = blobs.get(&placement.object.unwrap()).unwrap().unwrap();
        assert_eq!(String::from_utf8(body).unwrap(), card("+4"));
        assert!(list(&store, ACCOUNT).unwrap().is_empty());
    }

    /// Another app's row parking in the same drain does not fail a
    /// resolution that was applied: it is judged by its own row.
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    #[test]
    fn a_resolution_is_judged_by_its_own_row() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let conflicts = list(&store, ACCOUNT).unwrap();
        let conflict = find(conflicts.clone(), conflicts[0].id, None).unwrap();

        let flags = PimdirAction::SetFlags {
            seq: 424242,
            flags: PimdirFlags::default(),
            base: None,
        };
        PimdirProducer::open(dir.path(), "cardamum")
            .unwrap()
            .enqueue("contacts", &flags, None)
            .unwrap();

        assert_eq!(
            conflict
                .apply(dir.path(), ACCOUNT, card("+4").as_bytes())
                .unwrap(),
            Applied::Resolved(Vec::new()),
        );
    }

    /// A divergence whose remote body no run has fetched yet is listed, not
    /// resolvable.
    ///
    /// Every conflict passes through that state, and reading it as
    /// resolvable would hand `--prefer-remote` a side that is not there.
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

    /// A listed divergence prints the item's public id as a string, as
    /// himalaya, cardamum and calendula print a store id.
    #[test]
    fn a_listed_conflict_prints_its_id_as_a_string() {
        use crate::conflict::report::ConflictSummary;

        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let conflict = list(&store, ACCOUNT).unwrap().remove(0);

        let json = serde_json::to_value(ConflictSummary::from(&conflict)).unwrap();

        assert_eq!(json["id"], serde_json::json!(conflict.id.to_string()));
    }

    /// A body no parser reads is not a decision, whoever wrote it.
    ///
    /// A crashed partial write, or a half-finished template saved by hand,
    /// would otherwise replace a real card with something that is not one:
    /// the item keeps its link id and loses every field behind the identity.
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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
            PimdirStatus::Conflict,
            "the refusal leaves the divergence exactly as it was",
        );
        assert_eq!(
            placement.object,
            Some(blobs.hash(card("+2").as_bytes())),
            "and leaves the local side untouched",
        );
    }

    /// A resolution changing the item's `UID` resolves some other item.
    ///
    /// The bytes read as a card, so nothing structural catches it: the store
    /// would address the row by an identity its content no longer states, a
    /// frontend reading it as one contact and the server as another.
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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

    /// A settled body asks a performer for nothing: its `X-PIMDIR-` lines
    /// are removed before it is queued (pimdir STORAGE §18).
    #[cfg(any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    #[test]
    fn a_settled_body_lands_without_its_pimdir_instructions() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(dir.path());
        let blobs = store.blobs();

        let conflicts = list(&store, ACCOUNT).unwrap();
        let conflict = find(conflicts.clone(), conflicts[0].id, None).unwrap();

        let asking = card("+4").replace("END:VCARD", "X-PIMDIR-TEST:1\r\nEND:VCARD");
        conflict
            .apply(dir.path(), ACCOUNT, asking.as_bytes())
            .unwrap();

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_eq!(placement.object, Some(blobs.hash(card("+4").as_bytes())));
    }
}
