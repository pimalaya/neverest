//! # Notices
//!
//! The plain lines a text-mode command prints on stdout as it works, one a
//! step, before its result, none under `--json`, whose events say the same.
//! stderr is left to the logs.

use std::{
    fmt,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::terminal;

static SILENT: AtomicBool = AtomicBool::new(false);

/// Silences every notice, for a `--json` run.
pub fn silence() {
    SILENT.store(true, Ordering::Relaxed);
}

/// Whether notices print, which they do unless [`silence`]d.
pub fn printing() -> bool {
    !SILENT.load(Ordering::Relaxed)
}

/// Prints `line` on stdout unless [`silence`]d, flushed so a pipe reads it
/// as it comes; a line that cannot be printed stops nothing.
pub fn print(line: impl fmt::Display) {
    if printing() {
        let line = terminal::text(&line.to_string());
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}
