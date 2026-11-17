//! # Terminal text
//!
//! What a text-mode command prints holds what servers and other clients
//! wrote (collection names, ids, bodies, refusal reasons), so it goes through
//! pimalaya-cli's sanitizer line by line; JSON keeps every string exact.

use std::fmt;

use anyhow::Result;
use pimalaya_cli::{output::Bytes, printer::Printer, sanitize};
use serde::{Serialize, Serializer};

/// A printer defusing every text result and event of `0`.
pub struct TerminalPrinter<P>(pub P);

impl<P: Printer> Printer for TerminalPrinter<P> {
    fn out<T: fmt::Display + Serialize>(&mut self, data: T) -> Result<()> {
        self.0.out(Defused(data))
    }

    fn event<T: fmt::Display + Serialize>(&mut self, event: T) -> Result<()> {
        self.0.event(Defused(event))
    }

    fn out_bytes(&mut self, bytes: Bytes) -> Result<()> {
        self.0.out_bytes(bytes)
    }

    fn is_json(&self) -> bool {
        self.0.is_json()
    }
}

/// A value displayed defused and serialized as it is.
pub struct Defused<T>(pub T);

impl<T: fmt::Display> fmt::Display for Defused<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text(&self.0.to_string()))
    }
}

impl<T: Serialize> Serialize for Defused<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// `text` with the control and bidi characters of each line replaced, its
/// line breaks and tabs kept.
pub fn text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        for (index, cell) in line.split('\t').enumerate() {
            if index > 0 {
                out.push('\t');
            }
            out.push_str(&sanitize::text(cell));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use crate::terminal::text;

    /// An escape sequence a server wrote cannot drive the terminal, and the
    /// lines around it stay lines.
    #[test]
    fn a_line_keeps_its_breaks_and_loses_its_escapes() {
        let written =
            "BEGIN:VCARD\r\nFN:\u{1b}]52;c;cHduZWQ=\u{7}Jane\r\n\tfolded\r\nEND:VCARD\r\n";

        assert_eq!(
            text(written),
            "BEGIN:VCARD\nFN:\u{fffd}]52;c;cHduZWQ=\u{fffd}Jane\n\tfolded\nEND:VCARD\n"
        );
        assert_eq!(text("invoice\u{202e}fdp.exe"), "invoice\u{fffd}fdp.exe");
    }
}
