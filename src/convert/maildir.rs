//! One-shot Maildir(++) -> m2store converter.
//!
//! Walks a Maildir(++) tree, translates folder names (`.Work.Foo` ->
//! `Work/Foo`; root cur/new/tmp -> `INBOX`), and for each message
//! unions three flag sources (info-section letters, optional
//! `dovecot-keywords` extended letters, optional `X-Keywords:` /
//! `X-Label:` header) before storing the bytes into the destination
//! m2dir and writing the flag sidecar. Idempotent: skips messages
//! whose checksum already exists at the destination.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use io_email::flag::{Flag, IanaFlag};
use io_m2dir::{client::M2dirClient, entry::write_checksum, flag::M2dirFlags as M2Flags};
use log::{debug, info, warn};
use pimalaya_cli::printer::Printer;
use walkdir::WalkDir;

use crate::convert::cli::HeaderSource;

/// Per-folder statistics, accumulated as the walker visits messages.
#[derive(Debug, Default)]
struct Stats {
    converted: usize,
    skipped: usize,
    failed: usize,
}

impl Stats {
    fn add(&mut self, other: &Stats) {
        self.converted += other.converted;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }
}

/// Entry point for the `convert-maildir` subcommand.
pub fn run(
    printer: &mut impl Printer,
    source: PathBuf,
    destination: PathBuf,
    read_headers: Option<HeaderSource>,
) -> Result<()> {
    info!(
        "Converting {} -> {}",
        source.display(),
        destination.display()
    );

    let client = M2dirClient::new(destination.to_string_lossy().into_owned());
    client
        .init_store()
        .with_context(|| format!("init m2store at {}", destination.display()))?;

    let mut total = Stats::default();

    for folder in discover_maildir_folders(&source) {
        let mailbox = translate_mailbox_name(&source, &folder.path);
        info!(
            "Converting folder {} -> mailbox {}",
            folder.path.display(),
            mailbox
        );

        let stats =
            convert_folder(&client, &folder, &mailbox, read_headers).unwrap_or_else(|err| {
                warn!("Folder {} failed: {err:#}", folder.path.display());
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
    info!("{summary}");
    printer.out(format!("{summary}\n"))?;
    Ok(())
}

/// A discovered Maildir folder on disk: its root path and the optional
/// per-folder `dovecot-keywords` table (extended `a..z` letter ->
/// keyword text).
#[derive(Debug)]
struct MaildirFolder {
    path: PathBuf,
    dovecot_keywords: BTreeMap<char, String>,
}

/// Walks `root` and yields every directory that contains `cur/`,
/// `new/`, and `tmp/` subdirectories. The root itself is included when
/// it satisfies the layout.
fn discover_maildir_folders(root: &Path) -> Vec<MaildirFolder> {
    let mut folders = Vec::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Skip the maildir subdirectories themselves; we only want
        // their parent (the Maildir folder root).
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

/// Parses a per-folder `dovecot-keywords` file: each line is
/// `<index> <keyword>`. The numeric index maps to extended letters
/// `a..z` (`0` = `a`, `1` = `b`, ...). Missing or unreadable files
/// yield an empty map.
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

/// Translates a Maildir(++) folder path into an m2store mailbox name.
///
/// The root folder maps to `INBOX`. Children carry a leading `.` and
/// use `.` as the segment separator (`.Work.Foo` -> `Work/Foo`).
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

/// Converts every message in `folder.cur` and `folder.new`. Returns
/// counts; individual failures are logged and counted, never
/// propagated.
fn convert_folder(
    client: &M2dirClient,
    folder: &MaildirFolder,
    mailbox: &str,
    read_headers: Option<HeaderSource>,
) -> Result<Stats> {
    let m2dir = client
        .create_mailbox(mailbox)
        .with_context(|| format!("create mailbox {mailbox}"))?;

    // List existing destination checksums once so per-message
    // idempotency is O(1).
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
                warn!("read_dir {}: {err}", dir.display());
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
                    warn!("Message {} failed: {err:#}", path.display());
                    stats.failed += 1;
                }
            }
        }
    }

    Ok(stats)
}

#[derive(Debug)]
enum ConvertOutcome {
    Converted,
    Skipped,
}

/// Reads one Maildir message, unions its flag sources, optionally
/// strips the configured header, and writes the result to the
/// destination m2dir. Returns `Skipped` when the destination already
/// holds an entry with the same checksum (the union of flags is still
/// re-applied).
fn convert_message(
    client: &M2dirClient,
    m2dir: &io_m2dir::m2dir::M2dir,
    path: &Path,
    dovecot_keywords: &BTreeMap<char, String>,
    read_headers: Option<HeaderSource>,
    existing_checksums: &BTreeSet<String>,
) -> Result<ConvertOutcome> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;

    // Flags from the info-section letters in the filename.
    let filename_flags = parse_info_section(path, dovecot_keywords);

    // Optionally strip a header before computing the destination
    // checksum and storing the message; collect the keywords carried
    // in that header into the union.
    let (final_bytes, header_flags) = match read_headers {
        Some(HeaderSource::XKeywords) => strip_and_collect(&bytes, "X-Keywords", b','),
        Some(HeaderSource::XLabel) => strip_and_collect(&bytes, "X-Label", b' '),
        None => (bytes, BTreeSet::new()),
    };

    // Union all three flag sources.
    let mut union: BTreeSet<Flag> = BTreeSet::new();
    union.extend(filename_flags);
    union.extend(header_flags);

    // Compute the source checksum the same way io-m2dir does and
    // short-circuit when the destination already carries it.
    let mut checksum = String::new();
    write_checksum(&final_bytes, &mut checksum)
        .ok()
        .context("compute checksum")?;

    let outcome = if existing_checksums.contains(&checksum) {
        debug!(
            "Skipping {} (checksum {checksum} already present)",
            path.display()
        );
        ConvertOutcome::Skipped
    } else {
        let entry = client
            .store(m2dir.clone(), final_bytes)
            .with_context(|| format!("store {}", path.display()))?;
        debug!("Stored {} as {}", path.display(), entry.id());
        ConvertOutcome::Converted
    };

    // Locate the destination entry's id (whether just-stored or
    // pre-existing) by checksum, then set its flag sidecar to the
    // union. set_flags is idempotent, so reapplying on an existing
    // entry is safe.
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

/// Returns the id of the entry inside `m2dir` whose checksum matches
/// `checksum`, or `None` if none does.
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

/// Parses the info section (`:2,<letters>`) of a Maildir filename
/// into a set of shared [`Flag`]s. Standard letters map to canonical
/// IANA flags; extended `a..z` letters resolve via the per-folder
/// `dovecot-keywords` table when present, otherwise they are dropped.
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
/// canonical IANA shared [`Flag`]. Mirrors the (private) reverse
/// table in `io_email::maildir::convert`.
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

/// Returns a copy of `bytes` with every header line whose name
/// case-insensitively matches `header_name` removed, alongside the set
/// of flags parsed from those headers' values.
///
/// The headers are split off by hand: scan up to the first
/// CRLF-CRLF (or LF-LF) boundary, drop matching header lines together
/// with their continuation (folded) lines, and re-emit the rest plus
/// the unchanged body. This avoids re-serializing the message via
/// `mail-parser`, which would reflow whitespace and break byte
/// fidelity.
fn strip_and_collect(bytes: &[u8], header_name: &str, separator: u8) -> (Vec<u8>, BTreeSet<Flag>) {
    // Locate the headers/body boundary. RFC 5322 mandates CRLF, but
    // tolerate bare LF (used by some mbsync / local-edit pipelines).
    let boundary = find_header_boundary(bytes);
    let (header_bytes, body_bytes) = bytes.split_at(boundary);

    let mut out = Vec::with_capacity(bytes.len());
    let mut flags = BTreeSet::new();

    let mut idx = 0;
    while idx < header_bytes.len() {
        // Length of the current logical header (one starter line plus
        // any folded continuation lines beginning with WSP).
        let header_end = next_header_end(header_bytes, idx);
        let line = &header_bytes[idx..header_end];

        if header_matches(line, header_name) {
            // Collect keywords from the value half (after the first
            // `:`), then drop the header bytes entirely.
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

/// Returns the byte offset of the start of the message body within
/// `bytes`. The body begins immediately after the first empty header
/// line (CRLF CRLF or LF LF). Returns `bytes.len()` when no boundary
/// is found.
fn find_header_boundary(bytes: &[u8]) -> usize {
    if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
        return pos + 4;
    }
    if let Some(pos) = bytes.windows(2).position(|w| w == b"\n\n") {
        return pos + 2;
    }
    bytes.len()
}

/// Returns the byte offset one past the end of the logical header
/// starting at `start`. A header runs until the next line that does
/// not begin with a whitespace continuation byte.
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

/// Returns the byte offset one past the end of the line containing
/// `start`, i.e. just after the terminating LF (or end of input).
fn line_end(bytes: &[u8], start: usize) -> usize {
    let mut idx = start;
    while idx < bytes.len() && bytes[idx] != b'\n' {
        idx += 1;
    }
    if idx < bytes.len() { idx + 1 } else { idx }
}

/// Returns `true` when `line` starts a header whose name
/// case-insensitively equals `name`.
fn header_matches(line: &[u8], name: &str) -> bool {
    let Some(colon) = line.iter().position(|&b| b == b':') else {
        return false;
    };
    let observed = &line[..colon];
    observed.eq_ignore_ascii_case(name.as_bytes())
}

/// Returns the value half of a header line (after the first `:`).
fn header_value(line: &[u8]) -> Option<&[u8]> {
    let colon = line.iter().position(|&b| b == b':')?;
    Some(&line[colon + 1..])
}

/// Splits a header value into keywords, trims surrounding whitespace
/// (and trailing CRLF), and inserts each non-empty token into `flags`.
fn collect_keywords(value: &[u8], separator: u8, flags: &mut BTreeSet<Flag>) {
    // Tolerate bytes that are not strictly UTF-8 by lossy-decoding;
    // header values are 7-bit ASCII in practice but folded
    // continuations and pathological producers can drift.
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

#[cfg(test)]
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
