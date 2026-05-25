// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! `neverest convert <SOURCE> <TARGET>` command: converts mail between
//! supported on-disk formats. Each operand is a URL whose scheme
//! (`maildir`, `m2dir`) selects the format and whose path points to
//! the on-disk root.

use std::path::PathBuf;
#[cfg(feature = "m2dir")]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[cfg(feature = "m2dir")]
use anyhow::Context;
use anyhow::{Result, bail};
use clap::Parser;
#[cfg(feature = "m2dir")]
use io_email::flag::{Flag, IanaFlag};
#[cfg(feature = "m2dir")]
use io_m2dir::{client::M2dirClient, entry::write_checksum, flag::M2dirFlags as M2Flags};
#[cfg(feature = "m2dir")]
use log::{debug, info, trace, warn};
use pimalaya_cli::printer::Printer;
use url::Url;
#[cfg(feature = "m2dir")]
use walkdir::WalkDir;

/// Convert mail from one on-disk format to another.
///
/// Both operands are URLs whose scheme selects the format
/// (`maildir`, `m2dir`) and whose path points to the on-disk root.
/// Currently only `maildir` -> `m2dir` is implemented; the reverse is
/// reserved for a future release.
#[derive(Debug, Parser)]
pub struct ConvertCommand {
    /// Source URL (e.g. `maildir:///home/me/Mail`).
    #[arg(value_name = "SOURCE")]
    pub source: Url,

    /// Target URL (e.g. `m2dir:///home/me/m2`).
    #[arg(value_name = "TARGET")]
    pub target: Url,

    /// Read keywords from a per-message header on the source side
    /// (Maildir only).
    #[arg(long, value_enum, value_name = "HEADER")]
    pub read_headers: Option<HeaderSource>,
}

/// Per-message header source for Maildir keyword recovery.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HeaderSource {
    /// OfflineIMAP-style `X-Keywords: foo, bar` (comma-separated).
    XKeywords,
    /// Mutt / notmuch-style `X-Label: foo bar` (space-separated).
    XLabel,
}

impl ConvertCommand {
    pub fn execute(self, _printer: &mut impl Printer) -> Result<()> {
        let src = self.source.scheme();
        let tgt = self.target.scheme();

        match (src, tgt) {
            #[cfg(feature = "m2dir")]
            ("maildir", "m2dir") => maildir_to_m2dir(
                _printer,
                url_path(&self.source)?,
                url_path(&self.target)?,
                self.read_headers,
            ),
            #[cfg(not(feature = "m2dir"))]
            ("maildir", "m2dir") => {
                bail!("`convert maildir -> m2dir` requires the `m2dir` feature")
            }
            ("m2dir", "maildir") => {
                bail!("Conversion from `m2dir` to `maildir` is not yet supported")
            }
            (a, b) if a == b => bail!("Source and target schemes are both `{a}`"),
            (a, b) => bail!("Unsupported conversion: `{a}` -> `{b}`"),
        }
    }
}

/// Extracts the on-disk path from a `<scheme>:<path>` URL.
fn url_path(url: &Url) -> Result<PathBuf> {
    let raw = url.path();
    if raw.is_empty() {
        bail!("URL `{url}` carries no path");
    }
    Ok(PathBuf::from(raw))
}

/// Maildir(++) -> m2store one-shot conversion. Unions flag sources
/// (filename letters, dovecot keywords, optional header) and is
/// idempotent on re-run via the destination checksum index.
#[cfg(feature = "m2dir")]
fn maildir_to_m2dir(
    printer: &mut impl Printer,
    source: PathBuf,
    destination: PathBuf,
    read_headers: Option<HeaderSource>,
) -> Result<()> {
    info!(
        "converting {} -> {}",
        source.display(),
        destination.display()
    );

    let client = M2dirClient::new(destination.to_string_lossy().into_owned());
    client
        .init_store()
        .with_context(|| format!("Init m2store at {}", destination.display()))?;

    let mut total = Stats::default();

    for folder in discover_maildir_folders(&source) {
        let mailbox = translate_mailbox_name(&source, &folder.path);
        debug!("converting folder {} -> {mailbox}", folder.path.display());

        let stats =
            convert_folder(&client, &folder, &mailbox, read_headers).unwrap_or_else(|err| {
                warn!("folder {} failed: {err:#}", folder.path.display());
                Stats {
                    failed: 1,
                    ..Default::default()
                }
            });

        total.add(&stats);
    }

    let summary = format!(
        "Converted {} messages, skipped {} existing, failed {}",
        total.converted, total.skipped, total.failed
    );
    printer.out(format!("{summary}\n"))?;
    Ok(())
}

/// Per-folder conversion counters.
#[cfg(feature = "m2dir")]
#[derive(Debug, Default)]
struct Stats {
    converted: usize,
    skipped: usize,
    failed: usize,
}

#[cfg(feature = "m2dir")]
impl Stats {
    fn add(&mut self, other: &Stats) {
        self.converted += other.converted;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }
}

/// A discovered Maildir folder plus its optional `dovecot-keywords`
/// extended-letter table.
#[cfg(feature = "m2dir")]
#[derive(Debug)]
struct MaildirFolder {
    path: PathBuf,
    dovecot_keywords: BTreeMap<char, String>,
}

/// Walks `root` and yields every Maildir folder (directory holding
/// `cur/`, `new/`, `tmp/`).
#[cfg(feature = "m2dir")]
fn discover_maildir_folders(root: &Path) -> Vec<MaildirFolder> {
    let mut folders = Vec::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("cur") | Some("new") | Some("tmp")
        ) {
            continue;
        }
        if path.join("cur").is_dir() && path.join("new").is_dir() && path.join("tmp").is_dir() {
            let dovecot_keywords = read_dovecot_keywords(path);
            folders.push(MaildirFolder {
                path: path.to_path_buf(),
                dovecot_keywords,
            });
        }
    }

    folders
}

/// Parses `dovecot-keywords` (`<index> <keyword>` per line) into a
/// letter -> keyword table; missing/unreadable files yield empty.
#[cfg(feature = "m2dir")]
fn read_dovecot_keywords(folder: &Path) -> BTreeMap<char, String> {
    let path = folder.join("dovecot-keywords");
    let mut keywords = BTreeMap::new();

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return keywords,
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((idx, keyword)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(idx) = idx.trim().parse::<u32>() else {
            continue;
        };
        let keyword = keyword.trim();
        if keyword.is_empty() {
            continue;
        }
        if let Some(letter) = char::from_u32(b'a' as u32 + idx) {
            if letter.is_ascii_lowercase() {
                keywords.insert(letter, keyword.to_string());
            }
        }
    }

    keywords
}

/// Translates a Maildir(++) folder path into an m2store mailbox name
/// (root -> `INBOX`, `.Work.Foo` -> `Work/Foo`).
#[cfg(feature = "m2dir")]
fn translate_mailbox_name(root: &Path, folder: &Path) -> String {
    if folder == root {
        return String::from("INBOX");
    }

    let raw = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    let stripped = raw.strip_prefix('.').unwrap_or(raw);
    stripped.replace('.', "/")
}

/// Converts every message in `folder.cur` / `folder.new`; per-message
/// failures are logged and counted, never propagated.
#[cfg(feature = "m2dir")]
fn convert_folder(
    client: &M2dirClient,
    folder: &MaildirFolder,
    mailbox: &str,
    read_headers: Option<HeaderSource>,
) -> Result<Stats> {
    let m2dir = client
        .create_mailbox(mailbox)
        .with_context(|| format!("create mailbox {mailbox}"))?;

    let existing = client
        .list_entries(m2dir.clone())
        .with_context(|| format!("list existing entries in mailbox {mailbox}"))?;
    let existing_checksums: BTreeSet<String> = existing
        .into_iter()
        .map(|entry| entry.checksum().to_string())
        .collect();

    let mut stats = Stats::default();

    for subdir in ["cur", "new"] {
        let dir = folder.path.join(subdir);
        let iter = match fs::read_dir(&dir) {
            Ok(iter) => iter,
            Err(err) => {
                warn!("read_dir failed on {}: {err}", dir.display());
                continue;
            }
        };

        for entry in iter.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            match convert_message(
                client,
                &m2dir,
                &path,
                &folder.dovecot_keywords,
                read_headers,
                &existing_checksums,
            ) {
                Ok(ConvertOutcome::Converted) => stats.converted += 1,
                Ok(ConvertOutcome::Skipped) => stats.skipped += 1,
                Err(err) => {
                    warn!("message {} failed: {err:#}", path.display());
                    stats.failed += 1;
                }
            }
        }
    }

    Ok(stats)
}

#[cfg(feature = "m2dir")]
#[derive(Debug)]
enum ConvertOutcome {
    Converted,
    Skipped,
}

/// Reads one Maildir message, unions its flag sources, optionally
/// strips a header, then stores it; flags are re-applied even when
/// the message is `Skipped`.
#[cfg(feature = "m2dir")]
fn convert_message(
    client: &M2dirClient,
    m2dir: &io_m2dir::m2dir::M2dir,
    path: &Path,
    dovecot_keywords: &BTreeMap<char, String>,
    read_headers: Option<HeaderSource>,
    existing_checksums: &BTreeSet<String>,
) -> Result<ConvertOutcome> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;

    let filename_flags = parse_info_section(path, dovecot_keywords);

    let (final_bytes, header_flags) = match read_headers {
        Some(HeaderSource::XKeywords) => strip_and_collect(&bytes, "X-Keywords", b','),
        Some(HeaderSource::XLabel) => strip_and_collect(&bytes, "X-Label", b' '),
        None => (bytes, BTreeSet::new()),
    };

    let mut union: BTreeSet<Flag> = BTreeSet::new();
    union.extend(filename_flags);
    union.extend(header_flags);

    let mut checksum = String::new();
    write_checksum(&final_bytes, &mut checksum)
        .ok()
        .context("compute checksum")?;

    let outcome = if existing_checksums.contains(&checksum) {
        trace!("skipping {} ({checksum} already present)", path.display());
        ConvertOutcome::Skipped
    } else {
        let entry = client
            .store(m2dir.clone(), final_bytes)
            .with_context(|| format!("Store {}", path.display()))?;
        trace!("stored {} as {}", path.display(), entry.id());
        ConvertOutcome::Converted
    };

    if !union.is_empty()
        && let Some(entry_id) = locate_entry_by_checksum(client, m2dir, &checksum)?
    {
        let mut m2flags = M2Flags::default();
        for flag in &union {
            m2flags.insert(flag.raw());
        }
        client
            .set_flags(m2dir, &entry_id, m2flags)
            .with_context(|| format!("set flags on {entry_id}"))?;
    }

    Ok(outcome)
}

/// Returns the entry id matching `checksum`, or `None`.
#[cfg(feature = "m2dir")]
fn locate_entry_by_checksum(
    client: &M2dirClient,
    m2dir: &io_m2dir::m2dir::M2dir,
    checksum: &str,
) -> Result<Option<String>> {
    let entries = client
        .list_entries(m2dir.clone())
        .context("list entries for checksum lookup")?;

    for entry in entries {
        if entry.checksum() == checksum {
            return Ok(Some(entry.id().to_string()));
        }
    }
    Ok(None)
}

/// Parses the `:2,<letters>` info section of a Maildir filename;
/// extended `a..z` letters resolve via `dovecot_keywords` when present.
#[cfg(feature = "m2dir")]
fn parse_info_section(path: &Path, dovecot_keywords: &BTreeMap<char, String>) -> BTreeSet<Flag> {
    let mut flags = BTreeSet::new();

    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return flags;
    };

    let Some((_, info)) = name.rsplit_once(":2,") else {
        return flags;
    };

    for c in info.chars() {
        if let Some(flag) = flag_from_maildir_char(c) {
            flags.insert(flag);
        } else if c.is_ascii_lowercase()
            && let Some(keyword) = dovecot_keywords.get(&c)
        {
            flags.insert(Flag::from_raw(keyword));
        }
    }

    flags
}

/// Maps the six standard Maildir info-section letters to their
/// canonical IANA shared [`Flag`].
#[cfg(feature = "m2dir")]
fn flag_from_maildir_char(c: char) -> Option<Flag> {
    match c {
        'S' => Some(Flag::from_iana(IanaFlag::Seen)),
        'R' => Some(Flag::from_iana(IanaFlag::Answered)),
        'F' => Some(Flag::from_iana(IanaFlag::Flagged)),
        'D' => Some(Flag::from_iana(IanaFlag::Draft)),
        'T' => Some(Flag::from_iana(IanaFlag::Deleted)),
        'P' => Some(Flag::from_iana(IanaFlag::Forwarded)),
        _ => None,
    }
}

/// Strips every header line matching `header_name` (case-insensitive)
/// and returns the rewritten bytes plus the flags collected from the
/// stripped values; folded continuation lines are removed too.
#[cfg(feature = "m2dir")]
fn strip_and_collect(bytes: &[u8], header_name: &str, separator: u8) -> (Vec<u8>, BTreeSet<Flag>) {
    let boundary = find_header_boundary(bytes);
    let (header_bytes, body_bytes) = bytes.split_at(boundary);

    let mut out = Vec::with_capacity(bytes.len());
    let mut flags = BTreeSet::new();

    let mut idx = 0;
    while idx < header_bytes.len() {
        let header_end = next_header_end(header_bytes, idx);
        let line = &header_bytes[idx..header_end];

        if header_matches(line, header_name) {
            if let Some(value) = header_value(line) {
                collect_keywords(value, separator, &mut flags);
            }
        } else {
            out.extend_from_slice(line);
        }

        idx = header_end;
    }

    out.extend_from_slice(body_bytes);
    (out, flags)
}

/// Offset of the message body (after the first CRLF-CRLF or LF-LF);
/// `bytes.len()` when no boundary is found.
#[cfg(feature = "m2dir")]
fn find_header_boundary(bytes: &[u8]) -> usize {
    if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
        return pos + 4;
    }
    if let Some(pos) = bytes.windows(2).position(|w| w == b"\n\n") {
        return pos + 2;
    }
    bytes.len()
}

/// Offset one past the end of the logical header at `start`,
/// including folded continuation lines.
#[cfg(feature = "m2dir")]
fn next_header_end(bytes: &[u8], start: usize) -> usize {
    let mut idx = line_end(bytes, start);
    while idx < bytes.len() {
        let b = bytes[idx];
        if b == b' ' || b == b'\t' {
            idx = line_end(bytes, idx);
        } else {
            break;
        }
    }
    idx
}

/// Offset just after the LF terminating the line at `start`.
#[cfg(feature = "m2dir")]
fn line_end(bytes: &[u8], start: usize) -> usize {
    let mut idx = start;
    while idx < bytes.len() && bytes[idx] != b'\n' {
        idx += 1;
    }
    if idx < bytes.len() { idx + 1 } else { idx }
}

/// True when `line`'s header name case-insensitively equals `name`.
#[cfg(feature = "m2dir")]
fn header_matches(line: &[u8], name: &str) -> bool {
    let Some(colon) = line.iter().position(|&b| b == b':') else {
        return false;
    };
    let observed = &line[..colon];
    observed.eq_ignore_ascii_case(name.as_bytes())
}

/// Value half of a header line (after the first `:`).
#[cfg(feature = "m2dir")]
fn header_value(line: &[u8]) -> Option<&[u8]> {
    let colon = line.iter().position(|&b| b == b':')?;
    Some(&line[colon + 1..])
}

/// Splits a header value by `separator` and inserts each non-empty
/// trimmed token into `flags`.
#[cfg(feature = "m2dir")]
fn collect_keywords(value: &[u8], separator: u8, flags: &mut BTreeSet<Flag>) {
    let text = String::from_utf8_lossy(value);
    let sep = separator as char;

    for raw in text.split(sep) {
        let trimmed = raw.trim_matches(|c: char| c.is_whitespace());
        if trimmed.is_empty() {
            continue;
        }
        flags.insert(Flag::from_raw(trimmed));
    }
}

#[cfg(all(test, feature = "m2dir"))]
mod tests {
    use super::*;

    #[test]
    fn translate_root_to_inbox() {
        let root = Path::new("/tmp/maildir");
        assert_eq!(translate_mailbox_name(root, root), "INBOX");
    }

    #[test]
    fn translate_dot_prefixed_subfolder() {
        let root = Path::new("/tmp/maildir");
        let folder = root.join(".Work.Foo");
        assert_eq!(translate_mailbox_name(root, &folder), "Work/Foo");
    }

    #[test]
    fn parse_info_section_picks_standard_letters() {
        let path = Path::new("/tmp/cur/1234.M:2,RS");
        let flags = parse_info_section(path, &BTreeMap::new());
        assert!(flags.iter().any(Flag::is_seen));
        assert!(flags.iter().any(Flag::is_answered));
    }

    #[test]
    fn parse_info_section_resolves_dovecot_keywords() {
        let path = Path::new("/tmp/cur/1234.M:2,Sa");
        let mut table = BTreeMap::new();
        table.insert('a', String::from("custom"));
        let flags = parse_info_section(path, &table);
        assert!(flags.iter().any(Flag::is_seen));
        assert!(flags.iter().any(|f| f.raw() == "custom"));
    }

    #[test]
    fn strip_and_collect_removes_xkeywords_and_yields_flags() {
        let msg = b"Subject: hi\r\nX-Keywords: foo, bar\r\nFrom: a@b\r\n\r\nbody\r\n";
        let (out, flags) = strip_and_collect(msg, "X-Keywords", b',');
        let out_str = String::from_utf8(out).unwrap();
        assert!(!out_str.contains("X-Keywords"));
        assert!(out_str.contains("Subject: hi"));
        assert!(out_str.contains("From: a@b"));
        assert!(out_str.ends_with("\r\nbody\r\n"));

        let raws: BTreeSet<&str> = flags.iter().map(Flag::raw).collect();
        assert!(raws.contains("foo"));
        assert!(raws.contains("bar"));
    }

    #[test]
    fn strip_and_collect_handles_folded_header() {
        let msg = b"Subject: hi\r\nX-Keywords: foo,\r\n bar, baz\r\nFrom: a@b\r\n\r\nbody\r\n";
        let (out, flags) = strip_and_collect(msg, "X-Keywords", b',');
        let out_str = String::from_utf8(out).unwrap();
        assert!(!out_str.contains("X-Keywords"));
        assert!(out_str.contains("From: a@b"));

        let raws: BTreeSet<&str> = flags.iter().map(Flag::raw).collect();
        assert!(raws.contains("foo"));
        assert!(raws.contains("bar"));
        assert!(raws.contains("baz"));
    }
}
