//! # Offline sync engine
//!
//! The [io-pimdir](https://github.com/pimalaya/io-pimdir) engine over its
//! own store, replacing the hand-rolled 3-way diff of src/sync/.
//!
//! One [`PimdirSourceStore`] handle per source over the same files: `load`
//! projects that source's view of the shared hub and `write` absorbs the
//! engine's writes back, so cross-source propagation of items, flags and
//! deletions needs no hand-rolled cross-merge.
//!
//! Sources meet only inside a namespace: a hub collection id is
//! `<namespace>/<name>`, so a mail source and a contacts source under one
//! account, or two providers cached side by side, never share a collection.

use std::time::Instant;

use anyhow::{Result, anyhow};
use io_pimdir::{
    client::PimdirSourceStore,
    collection::PimdirCollectionId,
    coroutine::*,
    hub::PimdirSourceId,
    placement::PimdirHandle,
    remote::{PimdirEnumerated, PimdirListing, PimdirRemote},
};

pub mod capability;
pub mod create;
pub mod driver;
pub mod invitation;
pub mod pipe;
pub mod prof;
pub mod remote;
pub mod state;
pub mod storage;
pub mod submit;

/// The pimdir source id of a configured source: its name, verbatim.
///
/// The axis that distinguishes each source's bindings of one shared item. It is
/// the configured name and nothing derived, so renaming a source orphans its
/// bindings rather than quietly rebinding them.
pub fn source_id(name: &str) -> PimdirSourceId {
    PimdirSourceId(name.to_string())
}

/// Runs an io-pimdir coroutine to completion over borrowed seams.
///
/// io-pimdir's `PimdirSourceStore::run`, but borrowing the remote and timing
/// each yield for [`prof`], so the driver keeps its long-lived store handle
/// and client across the ephemeral coroutine.
pub fn run_verb<R, C, T, E>(
    store: &mut PimdirSourceStore,
    remote: &mut R,
    coroutine: C,
) -> Result<T>
where
    R: PimdirRemote,
    R::Error: std::fmt::Display,
    E: std::fmt::Display,
    C: PimdirCoroutine<Yield = PimdirYield, Return = Result<T, E>>,
{
    run_verb_paged(store, remote, coroutine, None)
}

/// Watches a verb's listing page by page.
///
/// Called where the store is at rest: no write of the verb is pending, and
/// nothing it loaded outlives the next page (the sync coroutine reloads what
/// a page names), so another verb may write the collection there.
pub trait Pager {
    /// The listing is about to ask for a page, or the verb completed.
    ///
    /// `landed` names the members of the page enumerated last, whose write
    /// has committed, and is empty before the first page. `resumed` is true
    /// once, before the first page of a round an earlier run left open.
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
    );

    /// The listing waits on the remote, the verb holding nothing.
    fn listing(&mut self);

    /// The remote answered, the verb about to take the page in.
    fn listed(&mut self);
}

/// [`run_verb`], telling `pager` each time a page of the listing has landed.
pub fn run_verb_paged<R, C, T, E>(
    store: &mut PimdirSourceStore,
    remote: &mut R,
    mut coroutine: C,
    mut pager: Option<&mut dyn Pager>,
) -> Result<T>
where
    R: PimdirRemote,
    R::Error: std::fmt::Display,
    E: std::fmt::Display,
    C: PimdirCoroutine<Yield = PimdirYield, Return = Result<T, E>>,
{
    let mut arg: Option<PimdirArg> = None;
    // NOTE: the page enumerated last, landed by the next enumerate or by the
    // completion: a page is written whole before the coroutine asks again.
    let mut page: Option<(PimdirCollectionId, Vec<PimdirHandle>)> = None;
    let mut first = true;

    loop {
        let yielded = match coroutine.resume(arg.take()) {
            PimdirCoroutineState::Complete(Ok(out)) => {
                if let (Some(pager), Some((collection, landed))) = (pager.as_deref_mut(), page) {
                    pager.at_rest(store, &collection, landed, false);
                }
                return Ok(out);
            }
            PimdirCoroutineState::Complete(Err(err)) => {
                return Err(anyhow!("Offline engine error: {err}"));
            }
            PimdirCoroutineState::Yielded(yielded) => yielded,
        };

        let stat = match &yielded {
            PimdirYield::WantsLoad { .. } => &prof::LOAD,
            PimdirYield::WantsLookupObject(_) => &prof::LOOKUP,
            PimdirYield::WantsWrite(_) => &prof::WRITE,
            PimdirYield::WantsEnumerate { .. } => &prof::ENUMERATE,
            PimdirYield::WantsFetch { .. } => &prof::FETCH,
            PimdirYield::WantsPush { .. } => &prof::PUSH,
        };
        let t = Instant::now();

        let serviced = store
            .service(yielded)
            .map_err(|err| anyhow!("Storage error: {err}"))?;

        arg = Some(match serviced {
            Ok(arg) => arg,
            Err(PimdirYield::WantsEnumerate {
                collection,
                request,
            }) => {
                if let Some(pager) = pager.as_deref_mut() {
                    let resumed = first
                        && matches!(
                            request.listing,
                            PimdirListing::Round {
                                cursor: Some(_),
                                ..
                            }
                        );
                    let landed = page.take().map(|(_, landed)| landed).unwrap_or_default();
                    pager.at_rest(store, &collection, landed, resumed);
                    pager.listing();
                }
                first = false;
                let listed = remote.enumerate(&collection, request);
                if let Some(pager) = pager.as_deref_mut() {
                    pager.listed();
                }
                let listed = listed.map_err(|err| anyhow!("Remote enumerate error: {err:#}"))?;
                if let PimdirEnumerated::Page(snapshot) = &listed {
                    let handles = snapshot.items.iter().map(|item| item.handle.clone());
                    page = Some((collection, handles.collect()));
                }
                PimdirArg::Enumerate(listed)
            }
            Err(PimdirYield::WantsFetch {
                collection,
                handles,
                tier,
            }) => PimdirArg::Fetch(
                remote
                    .fetch(&collection, handles, tier)
                    .map_err(|err| anyhow!("Remote fetch error: {err:#}"))?,
            ),
            Err(PimdirYield::WantsPush {
                collection,
                changes,
            }) => PimdirArg::Push(
                remote
                    .push(&collection, changes)
                    .map_err(|err| anyhow!("Remote push error: {err:#}"))?,
            ),
            Err(_) => unreachable!("a storage yield is serviced by the store"),
        });

        stat.add(t.elapsed());
    }
}
