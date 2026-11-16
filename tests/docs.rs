//! The documentation ceilings of the guidelines, checked over src/ and the
//! README: a `//!` paragraph of four lines at most (inline-001), a `///`
//! block of one line plus three after a blank (inline-002, src/cli/ aside
//! for clap's help), and the README's Features and section order.

use std::{
    fs,
    path::{Path, PathBuf},
};

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
            "License",
            "Social",
            "Sponsoring",
        ]
    );
}
