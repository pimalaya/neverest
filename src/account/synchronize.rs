//! `neverest synchronize` command.
//!
//! Wires the per-account [`crate::sync::builder::SyncBuilder`] up
//! against the indicatif progress UI ported from neverest v1
//! (`MultiProgress` + per-mailbox sub-bars). The dry-run path skips
//! every backend mutation and prints the planned patch instead.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use anyhow::{Result, bail};
use clap::{ArgAction, Parser};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use pimalaya_cli::printer::Printer;

use crate::cli::load_or_wizard;
use crate::config::MailboxFilter;
use crate::side::{Side, SideClient, take_account};
use crate::sync::builder::SyncBuilder;
use crate::sync::event::SyncEvent;
use crate::sync::hunk::EmailHunk;

static MAIN_PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template(" {spinner:.dim} {msg:.dim}\n {wide_bar:.cyan/blue} \n").unwrap()
});

static SUB_PROGRESS_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template(
        "   {prefix:.bold}: {wide_msg:.dim} \n   {wide_bar:.black/black} {percent}% ",
    )
    .unwrap()
});

static SUB_PROGRESS_DONE_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("   {prefix:.bold} \n   {wide_bar:.green} {percent}% ").unwrap()
});

/// Synchronize mailboxes and messages of the configured left and
/// right backends.
#[derive(Debug, Parser)]
pub struct SynchronizeAccountCommand {
    /// Optional account name. Defaults to the account marked
    /// `default = true` when omitted.
    #[arg(value_name = "ACCOUNT")]
    pub account: Option<String>,

    /// Run the synchronization without applying any changes.
    ///
    /// A report is printed to stdout describing the patch the
    /// synchronization would have applied.
    #[arg(long, short = 'd')]
    pub dry_run: bool,

    /// Synchronize only the given mailbox names. Repeat the flag for
    /// multiple names; conflicts with `--exclude-mailbox` and
    /// `--all-mailboxes`.
    #[arg(long, short = 'm')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "exclude_mailbox", conflicts_with = "all_mailboxes")]
    pub include_mailbox: Vec<String>,

    /// Skip the given mailbox names. Repeat the flag for multiple
    /// names; conflicts with `--include-mailbox` and `--all-mailboxes`.
    #[arg(long, short = 'x')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "all_mailboxes")]
    pub exclude_mailbox: Vec<String>,

    /// Synchronize every mailbox on both sides, ignoring any filter
    /// defined in the configuration file.
    #[arg(long, short = 'A')]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "exclude_mailbox")]
    pub all_mailboxes: bool,
}

impl SynchronizeAccountCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let config = load_or_wizard(config_paths)?;
        let (name, account, account_config) = take_account(config, self.account.as_deref())?;

        let left = SideClient::open(account_config.left.clone(), account.clone(), Side::Left)?;
        let right = SideClient::open(account_config.right.clone(), account, Side::Right)?;

        let cli_filter = if !self.include_mailbox.is_empty() {
            Some(MailboxFilter::Include(self.include_mailbox.clone()))
        } else if !self.exclude_mailbox.is_empty() {
            Some(MailboxFilter::Exclude(self.exclude_mailbox.clone()))
        } else if self.all_mailboxes {
            Some(MailboxFilter::All)
        } else {
            None
        };

        let builder = SyncBuilder::new(&name, &account_config, left, right)
            .with_mailbox_filter(cli_filter)
            .with_dry_run(self.dry_run);

        if self.dry_run {
            let report = builder.run(())?;
            let mut hunks_count = report.mailbox.patch.len();

            if !report.mailbox.patch.is_empty() {
                log_line("Mailboxes patch:\n")?;
                for (hunk, _) in &report.mailbox.patch {
                    log_line(format!(" - {hunk}\n"))?;
                }
                log_line("\n")?;
            }

            if !report.email.patch.is_empty() {
                log_line("Envelopes patch:\n")?;
                for (hunk, _) in &report.email.patch {
                    hunks_count += 1;
                    log_line(format!(" - {hunk}\n"))?;
                }
                log_line("\n")?;
            }

            printer.out(format!(
                "Estimated patch length for account {name} to be synchronized: {hunks_count}\n"
            ))?;
            return Ok(());
        }

        if printer.is_json() {
            builder.run(())?;
            printer.out(format!("Account {name} successfully synchronized!"))?;
            return Ok(());
        }

        let multi = MultiProgress::new();
        let sub_progresses: Mutex<HashMap<String, ProgressBar>> = Mutex::new(HashMap::new());
        let main_progress = multi.add(
            ProgressBar::new(100)
                .with_style(MAIN_PROGRESS_STYLE.clone())
                .with_message("Listing mailboxes\u{2026}"),
        );
        main_progress.tick();

        let multi_for_handler = multi.clone();
        let report = builder.run(move |evt: SyncEvent| -> Result<()> {
            match evt {
                SyncEvent::ListedAllMailboxes => {
                    main_progress.set_message("Synchronizing mailboxes\u{2026}");
                }
                SyncEvent::ProcessedAllMailboxHunks => {
                    main_progress.set_message("Listing envelopes\u{2026}");
                }
                SyncEvent::GeneratedEmailPatch(patches) => {
                    let total: usize = patches.values().map(Vec::len).sum();
                    main_progress.set_length(total as u64);
                    main_progress.set_position(0);
                    main_progress.set_message("Synchronizing messages\u{2026}");

                    let mut entries = sub_progresses.lock().unwrap();
                    for (mailbox, patch) in patches {
                        let bar = ProgressBar::new(patch.len() as u64)
                            .with_style(SUB_PROGRESS_STYLE.clone())
                            .with_prefix(mailbox.clone())
                            .with_finish(ProgressFinish::AndClear);
                        let bar = multi_for_handler.add(bar);
                        entries.insert(mailbox, bar);
                    }
                }
                SyncEvent::ProcessedEmailHunk(hunk) => {
                    main_progress.inc(1);
                    let mut entries = sub_progresses.lock().unwrap();
                    if let Some(bar) = entries.get_mut(hunk.mailbox()) {
                        bar.inc(1);
                        if let Some(len) = bar.length()
                            && bar.position() >= len.saturating_sub(1)
                        {
                            bar.set_style(SUB_PROGRESS_DONE_STYLE.clone());
                        } else {
                            bar.set_message(hunk_summary(&hunk));
                        }
                    }
                }
                SyncEvent::ProcessedAllEmailHunks => {
                    let mut entries = sub_progresses.lock().unwrap();
                    for bar in entries.values() {
                        bar.finish_and_clear();
                    }
                    entries.clear();

                    main_progress.set_length(100);
                    main_progress.set_position(100);
                    main_progress.set_message("Cleaning up\u{2026}");
                }
                SyncEvent::Done => {
                    main_progress.finish_and_clear();
                }
            }
            Ok(())
        })?;

        let mailbox_errs = collect_errors(&report.mailbox.patch);
        if !mailbox_errs.is_empty() {
            log_line("")?;
            log_line("Errors occurred while applying the mailboxes patch:")?;
            for (hunk, err) in mailbox_errs {
                log_line(format!(" - {hunk}: {err}"))?;
            }
        }

        let email_errs = collect_errors(&report.email.patch);
        if !email_errs.is_empty() {
            log_line("")?;
            log_line("Errors occurred while applying the envelopes patch:")?;
            for (hunk, err) in email_errs {
                log_line(format!(" - {hunk}: {err}"))?;
            }
        }

        printer.out(format!("Account {name} successfully synchronized!"))?;
        Ok(())
    }
}

/// Writes a decorative log line to stderr. Stays out of the JSON
/// stream `Printer` carries on stdout so machine-parseable callers
/// only see the structured summary.
fn log_line(msg: impl AsRef<str>) -> Result<()> {
    eprint!("{}", msg.as_ref());
    Ok(())
}

fn hunk_summary(hunk: &EmailHunk) -> String {
    match hunk {
        EmailHunk::Copy { source_id, .. } => format!("copy `{source_id}`"),
        EmailHunk::AddFlags { id, .. } => format!("add flags on `{id}`"),
        EmailHunk::Delete { id, .. } => format!("delete `{id}`"),
    }
}

fn collect_errors<H: std::fmt::Display>(
    patch: &[(H, Option<anyhow::Error>)],
) -> Vec<(&H, &anyhow::Error)> {
    patch
        .iter()
        .filter_map(|(hunk, err)| err.as_ref().map(|e| (hunk, e)))
        .collect()
}

#[allow(dead_code)]
fn ensure_not_empty<T>(slice: &[T], context: &str) -> Result<()> {
    if slice.is_empty() {
        bail!("{context} is empty");
    }
    Ok(())
}
