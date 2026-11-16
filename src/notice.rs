//! # Notices
//!
//! The plain lines a text-mode command prints on stderr as it works, one a
//! step, none under `--json`, whose events and result say the same.

use std::{
    fmt,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

static SILENT: AtomicBool = AtomicBool::new(false);

/// Silences every notice, for a `--json` run.
pub fn silence() {
    SILENT.store(true, Ordering::Relaxed);
}

/// Whether notices print, which they do unless [`silence`]d.
pub fn printing() -> bool {
    !SILENT.load(Ordering::Relaxed)
}

/// Prints `line` on stderr unless [`silence`]d; a line that cannot be
/// printed stops nothing.
pub fn print(line: impl fmt::Display) {
    if printing() {
        let _ = writeln!(io::stderr().lock(), "{line}");
    }
}
