//! # Sync engine
//!
//! The [io-pimdir](https://github.com/pimalaya/io-pimdir) engine over its own
//! store, and the report a run prints ([`hunk`], [`report`]).
//!
//! One [`PimdirSourceStore`] handle per source over the same files: `load`
//! projects its view of the hub, `write` absorbs the engine's writes back,
//! so items, flags and deletions cross sources with no cross-merge. Sources
//! meet only inside the namespace of a hub collection id, their source id.

pub mod capability;
pub mod collection;
pub mod driver;
pub mod event;
pub mod hunk;
#[cfg(feature = "search")]
pub mod index;
pub mod invitation;
pub mod local;
pub mod progress;
pub mod remote;
pub mod report;
pub mod state;
pub mod storage;
pub mod submit;

use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use io_pimdir::{
    client::PimdirSourceStore,
    codec::{self, PimdirPayloadOutcome},
    collection::PimdirCollectionId,
    coroutine::*,
    hub::PimdirSourceId,
    placement::PimdirHandle,
    remote::{PimdirEnumerated, PimdirListing, PimdirRemote},
};
use pimalaya_cli::signal::Shutdown;

/// The shutdown a sync polls, once `sync` installed it.
static SHUTDOWN: OnceLock<Shutdown> = OnceLock::new();

/// Makes the first SIGINT or SIGTERM ask the run to stop rather than end the
/// process, so it prints what it did (pimalaya-cli `signal::Shutdown`).
pub fn install_shutdown() -> Result<()> {
    if SHUTDOWN.get().is_none() {
        let _ = SHUTDOWN.set(Shutdown::install()?);
    }

    Ok(())
}

/// Whether a termination signal asked the run to stop.
///
/// The loops start no new collection, batch or account, and the account's
/// report reads as interrupted.
pub fn stopping() -> bool {
    SHUTDOWN.get().is_some_and(Shutdown::is_requested)
}

/// The pimdir source id of an endpoint, verbatim `<account>/<name>`
/// ([`crate::config::AccountConfig::namespaced`]).
///
/// The axis telling each source's bindings of one shared item apart; nothing
/// derived, so renaming a source or an account orphans its bindings rather
/// than quietly rebinding them.
pub fn source_id(name: &str) -> PimdirSourceId {
    PimdirSourceId(name.to_string())
}

/// Whether the payload of the queued intent `kind` states an integer `v`
/// this neverest holds no schema for (pimdir STORAGE §15.3).
///
/// Such a row is left pending and untouched for a neverest that knows the
/// version, never performed nor parked; a missing or non-integer `v` names
/// no version to wait for and is malformed instead.
pub fn states_unknown_version(kind: &str, payload: &str) -> bool {
    codec::payload_outcome(kind, payload) == PimdirPayloadOutcome::Unknown
}

/// Runs an io-pimdir coroutine to completion over borrowed seams.
///
/// io-pimdir's `PimdirSourceStore::run`, but borrowing the remote, so the
/// driver keeps its long-lived store handle and client across the
/// ephemeral coroutine.
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
    match run_verb_paged(store, remote, coroutine, None)? {
        Paged::Done(out) => Ok(out),
        Paged::Paused(_) => unreachable!("only a pager pauses a verb"),
    }
}

/// Whether a verb goes on listing, as its [`Pager`] answers at rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rest {
    /// The listing asks for its next page.
    Go,
    /// The verb stops where it stands, every page it listed committed.
    ///
    /// The store holds what a kill there would leave, a round open at its
    /// cursor, which the next sync of the collection resumes.
    Pause,
}

/// How a paged verb ended: completed, or paused by its pager, the
/// coroutine handed back so its partial report can be read.
pub enum Paged<T, C> {
    Done(T),
    Paused(C),
}

/// Watches a verb's listing page by page.
///
/// Called where the store is at rest: no write of the verb is pending, and
/// nothing it loaded outlives the next page (the sync coroutine reloads what
/// a page names), so another verb may write the collection there.
pub trait Pager {
    /// The listing is about to ask for `next`, or the verb completed (`None`).
    ///
    /// `landed` names the committed page enumerated last; `resumed` is true
    /// once, before a round an earlier run left open (only such a round lists
    /// a head, pimdir SYNC §5); [`Rest::Pause`] stops the verb before it asks.
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
        next: Option<&PimdirListing>,
    ) -> Rest;

    /// The listing waits on the remote, the verb holding nothing.
    fn listing(&mut self);

    /// The remote answered, the verb about to take the page in.
    fn listed(&mut self);
}

/// [`run_verb`], telling `pager` each time a page of the listing has landed,
/// and stopping where it answers [`Rest::Pause`].
///
/// A page is written whole, its pushes settled, before the coroutine asks
/// again, so dropping the coroutine there leaves what a kill would.
pub fn run_verb_paged<R, C, T, E>(
    store: &mut PimdirSourceStore,
    remote: &mut R,
    mut coroutine: C,
    mut pager: Option<&mut dyn Pager>,
) -> Result<Paged<T, C>>
where
    R: PimdirRemote,
    R::Error: std::fmt::Display,
    E: std::fmt::Display,
    C: PimdirCoroutine<Yield = PimdirYield, Return = Result<T, E>>,
{
    let mut arg: Option<PimdirArg> = None;
    let mut page: Option<(PimdirCollectionId, Vec<PimdirHandle>)> = None;
    let mut first = true;

    loop {
        let yielded = match coroutine.resume(arg.take()) {
            PimdirCoroutineState::Complete(Ok(out)) => {
                if let (Some(pager), Some((collection, landed))) = (pager.as_deref_mut(), page) {
                    pager.at_rest(store, &collection, landed, false, None);
                }
                return Ok(Paged::Done(out));
            }
            PimdirCoroutineState::Complete(Err(err)) => {
                return Err(anyhow!("Offline engine error: {err}"));
            }
            PimdirCoroutineState::Yielded(yielded) => yielded,
        };

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
                            } | PimdirListing::Head(_)
                        );
                    let landed = page.take().map(|(_, landed)| landed).unwrap_or_default();
                    let next = Some(&request.listing);
                    if pager.at_rest(store, &collection, landed, resumed, next) == Rest::Pause {
                        return Ok(Paged::Paused(coroutine));
                    }
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
    }
}
