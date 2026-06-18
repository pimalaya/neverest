//! Bidirectional 3-way sync: diff math, hunk types, state snapshot,
//! worker pool, and the `run` entry point.

mod sync;

pub mod diff;
pub mod hunk;
pub mod pool;
pub mod report;
pub mod state;

pub use sync::*;
