//! # Drive-loop profiling
//!
//! Set `NEVEREST_PROFILE=1` to accumulate, per io-replica yield kind, the call
//! count and wall time spent servicing it. The breakdown is printed to stderr
//! at the end of a sync run.
//!
//! Temporary diagnostic scaffolding, not a product feature.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

/// One accumulator: a yield kind's service count and total nanoseconds.
pub struct Stat {
    /// How many times that kind was serviced.
    pub count: AtomicU64,
    /// How long servicing it took, summed.
    pub nanos: AtomicU64,
}

impl Stat {
    const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
        }
    }
    /// Records one service of that kind and what it cost.
    pub fn add(&self, dur: Duration) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.nanos
            .fetch_add(dur.as_nanos() as u64, Ordering::Relaxed);
    }
    fn read(&self) -> (u64, f64) {
        (
            self.count.load(Ordering::Relaxed),
            self.nanos.load(Ordering::Relaxed) as f64 / 1e9,
        )
    }
}

/// Time spent reading placements out of the store.
pub static LOAD: Stat = Stat::new();
/// Time spent writing the engine's own batches back.
pub static WRITE: Stat = Stat::new();
/// Time spent listing a collection on a remote.
pub static ENUMERATE: Stat = Stat::new();
/// Time spent fetching bodies.
pub static FETCH: Stat = Stat::new();
/// Time spent pushing changes to a remote.
pub static PUSH: Stat = Stat::new();
/// Time spent resolving link ids against the store.
pub static LOOKUP: Stat = Stat::new();

/// Whether profiling is enabled (`NEVEREST_PROFILE` set to a non-empty value).
pub fn enabled() -> bool {
    std::env::var_os("NEVEREST_PROFILE").is_some_and(|v| !v.is_empty())
}

/// Prints the accumulated breakdown to stderr.
pub fn report() {
    if !enabled() {
        return;
    }
    eprintln!("--- neverest drive profile (count / seconds) ---");
    for (name, stat) in [
        ("load    ", &LOAD),
        ("write   ", &WRITE),
        ("enumerate", &ENUMERATE),
        ("fetch   ", &FETCH),
        ("push    ", &PUSH),
        ("lookup  ", &LOOKUP),
    ] {
        let (count, secs) = stat.read();
        eprintln!("  {name}  {count:>8}  {secs:>10.3}s");
    }
}
