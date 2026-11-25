//! # Documentation tests
//!
//! The documentation ceilings of the guidelines, checked over src/ and the
//! README: a `//!` paragraph of four lines at most (inline-001), a `///`
//! block of one line plus three after a blank (inline-002, src/cli/ aside
//! for clap's help), the README's Features and section order, and its
//! terminal block printing what it shows (readme-006).

#[cfg(feature = "carddav")]
mod fakedav;

#[cfg(feature = "carddav")]
use std::process::{Command, Stdio};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(feature = "carddav")]
use crate::fakedav::FakeDav;

/// Every Rust file under `dir`, in path order.
fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

/// The runs of consecutive lines starting with `prefix`, each as its first
/// line number and its lines with the prefix stripped.
fn blocks<'a>(text: &'a str, prefix: &str) -> Vec<(usize, Vec<&'a str>)> {
    let mut blocks: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut open = false;

    for (number, line) in text.lines().enumerate() {
        let Some(doc) = line.trim_start().strip_prefix(prefix) else {
            open = false;
            continue;
        };

        if !open {
            blocks.push((number + 1, Vec::new()));
            open = true;
        }

        blocks.last_mut().unwrap().1.push(doc.trim());
    }

    blocks
}

/// The paragraphs of a doc block, code fences and lists left out.
fn paragraphs<'a>(lines: &[&'a str]) -> Vec<Vec<&'a str>> {
    let mut paragraphs = vec![Vec::new()];
    let mut fenced = false;

    for line in lines {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }

        if fenced || line.starts_with("- ") || line.starts_with("| ") {
            continue;
        }

        if line.is_empty() {
            paragraphs.push(Vec::new());
        } else {
            paragraphs.last_mut().unwrap().push(*line);
        }
    }

    paragraphs.retain(|paragraph| !paragraph.is_empty());
    paragraphs
}

#[test]
fn a_module_header_paragraph_holds_four_lines_at_most() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut long = Vec::new();

    for path in sources(&root.join("src")) {
        let text = fs::read_to_string(&path).unwrap();
        for (line, block) in blocks(&text, "//!") {
            for paragraph in paragraphs(&block) {
                if paragraph.len() > 4 {
                    long.push(format!("{}:{line}: {}", path.display(), paragraph[0]));
                }
            }
        }
    }

    assert!(
        long.is_empty(),
        "{} paragraphs:\n{}",
        long.len(),
        long.join("\n")
    );
}

#[test]
fn an_item_doc_holds_a_summary_and_three_lines_at_most() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cli = root.join("src").join("cli");
    let mut long = Vec::new();

    for path in sources(&root.join("src")) {
        if path.starts_with(&cli) {
            continue;
        }

        let text = fs::read_to_string(&path).unwrap();
        for (line, block) in blocks(&text, "///") {
            let paragraphs = paragraphs(&block);
            let summary = paragraphs.first().map_or(0, Vec::len);
            let rest: usize = paragraphs.iter().skip(1).map(Vec::len).sum();

            if summary > 2 || rest > 3 {
                long.push(format!("{}:{line}: {}", path.display(), block[0]));
            }
        }
    }

    assert!(long.is_empty(), "{} docs:\n{}", long.len(), long.join("\n"));
}

/// The README's `## ` sections, in order.
fn readme_sections(readme: &str) -> Vec<&str> {
    readme
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect()
}

#[test]
fn the_readme_features_are_eight_short_bullets_at_most() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    let features: Vec<&str> = readme
        .split("\n## Features\n")
        .nth(1)
        .expect("a Features section")
        .split("\n## ")
        .next()
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("- "))
        .collect();

    assert!(!features.is_empty() && features.len() <= 8, "{features:#?}");
    for feature in features {
        assert!(feature.chars().count() <= 120, "{feature}");
    }
}

#[test]
fn the_readme_sections_follow_the_cli_order() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();

    assert_eq!(
        readme_sections(&readme),
        [
            "Table of contents",
            "Features",
            "Installation",
            "Configuration",
            "Usage",
            "Alternatives",
            "License",
            "Social",
            "Sponsoring",
        ]
    );
}

/// The content of the first fenced block of `lang` in `text`, and the
/// rest of `text` after it.
#[cfg(feature = "carddav")]
fn fenced<'a>(text: &'a str, lang: &str) -> (&'a str, &'a str) {
    let open = format!("```{lang}\n");
    let start = text.find(&open).expect("a fenced block") + open.len();
    let end = start + text[start..].find("\n```").expect("a closing fence");
    (&text[start..end], &text[end + 4..])
}

/// The command line under the header, run as the first sync of an account
/// holding one book of two cards on a fake CardDAV server, prints the text
/// shown after it.
#[cfg(feature = "carddav")]
#[test]
fn the_readme_header_block_prints_what_it_shows() {
    let readme = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    let (command, rest) = fenced(&readme, "sh");
    let (shown, _) = fenced(rest, "text");
    let args: Vec<&str> = command
        .strip_prefix("$ neverest ")
        .expect("a neverest command line")
        .split_whitespace()
        .collect();

    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put(
        "ada.vcf",
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n",
    );
    dav.put(
        "alan.vcf",
        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:alan\r\nFN:Alan\r\nEND:VCARD\r\n",
    );

    let store = home.path().join("store").display().to_string();
    let toml = format!(
        "[accounts.personal]\n\
         store.root = {store:?}\n\
         carddav.home = {dav:?}\n\
         carddav.auth.basic.username = \"u\"\n\
         carddav.auth.basic.password.raw = \"p\"\n",
        dav = dav.home,
    );
    let config = home.path().join("config.toml");
    fs::write(&config, toml).unwrap();

    let neverest = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_neverest"))
            .arg("-c")
            .arg(&config)
            .args(args)
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .env("XDG_STATE_HOME", home.path())
            .env_remove("XDG_CONFIG_DIRS")
            .env_remove("RUST_LOG")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    assert!(neverest(&["init"]).status.success());
    let out = neverest(&args);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert_eq!(stdout.trim_end(), shown);
}
