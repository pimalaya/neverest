//! # Offline sync engine
//!
//! The [io-replica](https://github.com/pimalaya/io-replica) engine over an
//! [io-pimdir](https://github.com/pimalaya/io-pimdir) store, replacing the
//! hand-rolled 3-way diff of src/sync/.
//!
//! One [`PimdirSourceStore`](io_pimdir::PimdirSourceStore) handle per source
//! over the same files: `load` projects that source's view of the shared hub
//! and `write` absorbs the engine's writes back, so cross-source propagation of
//! items, flags and deletions needs no hand-rolled cross-merge.
//!
//! Sources meet only inside a namespace: a hub collection id is
//! `<namespace>/<name>`, so a mail source and a contacts source under one
//! account, or two providers cached side by side, never share a collection.

use anyhow::{Result, anyhow};
use io_replica::{
    client::{ReplicaRemote, ReplicaStorage},
    coroutine::{ReplicaArg, ReplicaCoroutine, ReplicaCoroutineState, ReplicaYield},
    hub::ReplicaSourceId,
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
pub fn source_id(name: &str) -> ReplicaSourceId {
    ReplicaSourceId(name.to_string())
}

/// Drives an io-replica coroutine to completion over borrowed seams.
///
/// io-replica's `ReplicaClient::run`, but borrowing, so the driver keeps its
/// long-lived per-side [`PimdirSourceStore`](io_pimdir::PimdirSourceStore)
/// handle and client across the ephemeral coroutine.
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
                    .map_err(|err| anyhow!("Remote enumerate error: {err:#}"))?;
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
                    .map_err(|err| anyhow!("Remote fetch error: {err:#}"))?;
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
                    .map_err(|err| anyhow!("Remote push error: {err:#}"))?;
                prof::PUSH.add(t.elapsed());
                arg = Some(ReplicaArg::Push(results));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLoad { collection, scope }) => {
                let t = std::time::Instant::now();
                let loaded = storage
                    .load(&collection, &scope)
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
