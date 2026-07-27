//! The io-replica-based sync engine, persisting to a pimdir store.
//!
//! Replaces the hand-rolled 3-way diff/apply of `src/sync/` with the
//! [io-replica](https://github.com/pimalaya/io-replica) replica engine over an
//! [io-pimdir](https://github.com/pimalaya/io-pimdir) store (a SQLite index plus
//! a content-addressed blob directory).
//!
//! A collection's two sides are the two *sources* of one shared collection in the
//! store: one [`PimdirStore`](io_pimdir::PimdirStore) handle per side (`"left"`
//! / `"right"`) over the same files, the collection name as the bare collection id.
//! `load` projects that source's view of the shared hub, `write` absorbs the
//! engine's writes back into that source's bindings, so cross-side propagation
//! of items, flags and deletions falls out of the per-side reconcile with no
//! hand-rolled cross-merge.
//!
//! Layout:
//! - [`storage`] the per-side projection and hydration helpers over a
//!   [`PimdirStore`](io_pimdir::PimdirStore);
//! - [`remote`] [`remote::PimRemote`], the [`ReplicaRemote`] over one client;
//! - [`driver`] per-account/per-collection orchestration and the report.

use anyhow::{Result, anyhow};
use io_replica::{
    client::{ReplicaRemote, ReplicaStorage},
    coroutine::{ReplicaArg, ReplicaCoroutine, ReplicaCoroutineState, ReplicaYield},
    hub::ReplicaSourceId,
};

use crate::side::Side;

pub mod driver;
pub mod pipe;
pub mod prof;
pub mod remote;
pub mod storage;
pub mod submit;

/// The pimdir source id for a side: the axis that distinguishes the two sides'
/// bindings of one shared item in the store. `"left"` / `"right"` match the
/// pimdir spec's documented source examples.
pub fn source_id(side: Side) -> ReplicaSourceId {
    match side {
        Side::Left => ReplicaSourceId("left".to_string()),
        Side::Right => ReplicaSourceId("right".to_string()),
    }
}

/// Drives any standard-shape io-replica coroutine to completion over borrowed
/// storage and remote seams (io-replica's `ReplicaClient::run`, but borrowing so
/// the driver keeps its long-lived per-side
/// [`PimdirStore`](io_pimdir::PimdirStore) handle and client across the
/// ephemeral coroutine).
pub fn drive<S, R, C, T, E>(storage: &mut S, remote: &mut R, mut coroutine: C) -> Result<T>
where
    S: ReplicaStorage,
    S::Error: std::fmt::Display,
    R: ReplicaRemote,
    R::Error: std::fmt::Display,
    E: std::fmt::Display,
    C: ReplicaCoroutine<Yield = ReplicaYield, Return = Result<T, E>>,
{
    let mut arg: Option<ReplicaArg> = None;

    loop {
        match coroutine.resume(arg.take()) {
            ReplicaCoroutineState::Complete(Ok(out)) => return Ok(out),
            ReplicaCoroutineState::Complete(Err(err)) => {
                return Err(anyhow!("Offline engine error: {err}"));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsEnumerate { collection, cursor }) => {
                let t = std::time::Instant::now();
                let snapshot = remote
                    .enumerate(&collection, cursor)
                    .map_err(|err| anyhow!("Remote enumerate error: {err}"))?;
                prof::ENUMERATE.add(t.elapsed());
                arg = Some(ReplicaArg::Enumerate(snapshot));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsFetch {
                collection,
                handles,
                tier,
            }) => {
                let t = std::time::Instant::now();
                let items = remote
                    .fetch(&collection, handles, tier)
                    .map_err(|err| anyhow!("Remote fetch error: {err}"))?;
                prof::FETCH.add(t.elapsed());
                arg = Some(ReplicaArg::Fetch(items));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsPush {
                collection,
                changes,
            }) => {
                let t = std::time::Instant::now();
                let results = remote
                    .push(&collection, changes)
                    .map_err(|err| anyhow!("Remote push error: {err}"))?;
                prof::PUSH.add(t.elapsed());
                arg = Some(ReplicaArg::Push(results));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLoad(collection)) => {
                let t = std::time::Instant::now();
                let loaded = storage
                    .load(&collection)
                    .map_err(|err| anyhow!("Storage load error: {err}"))?;
                prof::LOAD.add(t.elapsed());
                arg = Some(ReplicaArg::Load(loaded));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLookupObject(links)) => {
                let t = std::time::Instant::now();
                let known = storage
                    .lookup_objects(&links)
                    .map_err(|err| anyhow!("Storage lookup error: {err}"))?;
                prof::LOOKUP.add(t.elapsed());
                arg = Some(ReplicaArg::LookupObject(known));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsWrite(ops)) => {
                let t = std::time::Instant::now();
                storage
                    .write(ops)
                    .map_err(|err| anyhow!("Storage write error: {err}"))?;
                prof::WRITE.add(t.elapsed());
                arg = Some(ReplicaArg::Write);
            }
        }
    }
}
