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
    client::PimdirSourceStore, coroutine::*, hub::PimdirSourceId, remote::PimdirRemote,
};

pub mod driver;
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
    mut coroutine: C,
) -> Result<T>
where
    R: PimdirRemote,
    R::Error: std::fmt::Display,
    E: std::fmt::Display,
    C: PimdirCoroutine<Yield = PimdirYield, Return = Result<T, E>>,
{
    let mut arg: Option<PimdirArg> = None;

    loop {
        let yielded = match coroutine.resume(arg.take()) {
            PimdirCoroutineState::Complete(Ok(out)) => return Ok(out),
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
            Err(PimdirYield::WantsEnumerate { collection, cursor }) => PimdirArg::Enumerate(
                remote
                    .enumerate(&collection, cursor)
                    .map_err(|err| anyhow!("Remote enumerate error: {err:#}"))?,
            ),
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
