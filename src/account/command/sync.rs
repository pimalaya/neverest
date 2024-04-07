//! # Synchronize account command
//!
//! This module contains the [`clap`] command for synchronizing two
//! backends of a given account.

use anyhow::Result;
use clap::{ArgAction, Parser};
#[cfg(feature = "imap")]
use email::imap::ImapContextBuilder;
#[cfg(feature = "maildir")]
use email::maildir::MaildirContextBuilder;
#[cfg(feature = "notmuch")]
use email::notmuch::NotmuchContextBuilder;
use email::{
    account::config::AccountConfig,
    backend::{context::BackendContextBuilder, BackendBuilder},
    folder::sync::config::FolderSyncStrategy,
    sync::{hash::SyncHash, SyncBuilder, SyncEvent},
};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::info;
use once_cell::sync::Lazy;
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
};

use crate::{
    account::arg::name::OptionalAccountNameArg, backend::config::BackendConfig, config::Config,
    printer::Printer,
};

static MAIN_PROGRESS_STYLE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template(" {spinner:.dim} {msg:.dim}\n {wide_bar:.cyan/blue} \n").unwrap()
});

static SUB_PROGRESS_STYLE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template(
        "   {prefix:.bold} — {wide_msg:.dim} \n   {wide_bar:.black/black} {percent}% ",
    )
    .unwrap()
});

static SUB_PROGRESS_DONE_STYLE: Lazy<ProgressStyle> = Lazy::new(|| {
    ProgressStyle::with_template("   {prefix:.bold} \n   {wide_bar:.green} {percent}% ").unwrap()
});

/// Synchronize folders and emails of two different backend sources.
#[derive(Debug, Parser)]
pub struct SynchronizeAccountCommand {
    #[command(flatten)]
    pub account: OptionalAccountNameArg,

    /// Run the synchronization without applying any changes.
    ///
    /// Instead, a report will be printed to stdout containing all the
    /// changes the synchronization plan to do.
    #[arg(long, short)]
    pub dry_run: bool,

    /// Synchronize only specific folders.
    ///
    /// Only the given folders will be synchronized (including
    /// associated envelopes and messages). Useful when you need to
    /// speed up the synchronization process. A good usecase is to
    /// synchronize only the INBOX in order to quickly check for new
    /// messages.
    #[arg(long, short = 'f')]
    #[arg(value_name = "FOLDER", action = ArgAction::Append)]
    #[arg(conflicts_with = "exclude_folder", conflicts_with = "all_folders")]
    pub include_folder: Vec<String>,

    /// Omit specific folders from the synchronization.
    ///
    /// The given folders will be excluded from the synchronization
    /// (including associated envelopes and messages). Useful when you
    /// have heavy folders that you do not want to take care of, or to
    /// speed up the synchronization process.
    #[arg(long, short = 'x')]
    #[arg(value_name = "FOLDER", action = ArgAction::Append)]
    #[arg(conflicts_with = "include_folder", conflicts_with = "all_folders")]
    pub exclude_folder: Vec<String>,

    /// Synchronize all exsting folders.
    #[arg(long, short = 'A')]
    #[arg(conflicts_with = "include_folder", conflicts_with = "exclude_folder")]
    pub all_folders: bool,
}

impl SynchronizeAccountCommand {
    pub async fn execute(self, printer: &mut impl Printer, config: &Config) -> Result<()> {
        info!("executing synchronize backends command");

        let (name, config) = config.into_account_config(self.account.name.as_deref())?;

        let folder_filter = config.folder.map(|c| c.filter).unwrap_or_default();
        let envelope_filter = config.envelope.map(|c| c.filter).unwrap_or_default();

        let (left_backend, left_config) = config.left.into_account_config(
            name.clone(),
            folder_filter.clone(),
            envelope_filter.clone(),
        );

        let (right_backend, right_config) =
            config
                .right
                .into_account_config(name.clone(), folder_filter, envelope_filter);

        match left_backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(imap_config) => {
                let left_ctx = ImapContextBuilder::new(left_config.clone(), Arc::new(imap_config))
                    .with_prebuilt_credentials()
                    .await?;
                let left = BackendBuilder::new(left_config.clone(), left_ctx);
                self.pre_sync(printer, name.as_str(), left, right_config, right_backend)
                    .await
            }
            #[cfg(feature = "maildir")]
            BackendConfig::Maildir(maildir_config) => {
                let left_ctx =
                    MaildirContextBuilder::new(left_config.clone(), Arc::new(maildir_config));
                let left = BackendBuilder::new(left_config.clone(), left_ctx);
                self.pre_sync(printer, name.as_str(), left, right_config, right_backend)
                    .await
            }
            #[cfg(feature = "notmuch")]
            BackendConfig::Notmuch(notmuch_config) => {
                let left_ctx =
                    NotmuchContextBuilder::new(left_config.clone(), Arc::new(notmuch_config));
                let left = BackendBuilder::new(left_config.clone(), left_ctx);
                self.pre_sync(printer, name.as_str(), left, right_config, right_backend)
                    .await
            }
        }
    }

    async fn pre_sync(
        self,
        printer: &mut impl Printer,
        account_name: &str,
        left: BackendBuilder<impl BackendContextBuilder + SyncHash + 'static>,
        right_config: Arc<AccountConfig>,
        right_backend: BackendConfig,
    ) -> Result<()> {
        match right_backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(imap_config) => {
                let right_ctx =
                    ImapContextBuilder::new(right_config.clone(), Arc::new(imap_config))
                        .with_prebuilt_credentials()
                        .await?;
                let right = BackendBuilder::new(right_config.clone(), right_ctx);
                self.sync(printer, account_name, left, right).await
            }
            #[cfg(feature = "maildir")]
            BackendConfig::Maildir(maildir_config) => {
                let right_ctx =
                    MaildirContextBuilder::new(right_config.clone(), Arc::new(maildir_config));
                let right = BackendBuilder::new(right_config.clone(), right_ctx);
                self.sync(printer, account_name, left, right).await
            }
            #[cfg(feature = "notmuch")]
            BackendConfig::Notmuch(notmuch_config) => {
                let right_ctx =
                    NotmuchContextBuilder::new(right_config.clone(), Arc::new(notmuch_config));
                let right = BackendBuilder::new(right_config.clone(), right_ctx);
                self.sync(printer, account_name, left, right).await
            }
        }
    }

    async fn sync(
        self,
        printer: &mut impl Printer,
        account_name: &str,
        left: BackendBuilder<impl BackendContextBuilder + SyncHash + 'static>,
        right: BackendBuilder<impl BackendContextBuilder + SyncHash + 'static>,
    ) -> Result<()> {
        let included_folders = BTreeSet::from_iter(self.include_folder);
        let excluded_folders = BTreeSet::from_iter(self.exclude_folder);

        let folders_filter = if !included_folders.is_empty() {
            Some(FolderSyncStrategy::Include(included_folders))
        } else if !excluded_folders.is_empty() {
            Some(FolderSyncStrategy::Exclude(excluded_folders))
        } else if self.all_folders {
            Some(FolderSyncStrategy::All)
        } else {
            None
        };

        let sync_builder = SyncBuilder::new(left, right).with_some_folders_filter(folders_filter);

        if self.dry_run {
            let report = sync_builder.with_dry_run(true).sync().await?;
            let mut hunks_count = report.folder.patch.len();

            if !report.folder.patch.is_empty() {
                printer.print_log("Folders patch:")?;
                for (hunk, _) in report.folder.patch {
                    printer.print_log(format!(" - {hunk}"))?;
                }
                printer.print_log("")?;
            }

            if !report.email.patch.is_empty() {
                printer.print_log("Envelopes patch:")?;
                for (hunk, _) in report.email.patch {
                    hunks_count += 1;
                    printer.print_log(format!(" - {hunk}"))?;
                }
                printer.print_log("")?;
            }

            printer.print(format!(
                "Estimated patch length for account {account_name} to be synchronized: {hunks_count}"
            ))?;
        } else if printer.is_json() {
            sync_builder.sync().await?;
            printer.print(format!("Account {account_name} successfully synchronized!"))?;
        } else {
            let multi = MultiProgress::new();
            let sub_progresses = Mutex::new(HashMap::new());
            let main_progress = multi.add(
                ProgressBar::new(100)
                    .with_style(MAIN_PROGRESS_STYLE.clone())
                    .with_message("Listing folders…"),
            );

            main_progress.tick();

            let report = sync_builder
                .with_handler(move |evt| {
                    match evt {
                        SyncEvent::ListedAllFolders => {
                            main_progress.set_message("Synchronizing folders…");
                        }
                        SyncEvent::ProcessedAllFolderHunks => {
                            main_progress.set_message("Listing envelopes…");
                        }
                        SyncEvent::GeneratedEmailPatch(patches) => {
                            let patches_len = patches.values().flatten().count();
                            main_progress.set_length(patches_len as u64);
                            main_progress.set_position(0);
                            main_progress.set_message("Synchronizing emails…");

                            let mut envelopes_progresses = sub_progresses.lock().unwrap();
                            for (folder, patch) in patches {
                                let progress = ProgressBar::new(patch.len() as u64)
                                    .with_style(SUB_PROGRESS_STYLE.clone())
                                    .with_prefix(folder.clone())
                                    .with_finish(ProgressFinish::AndClear);
                                let progress = multi.add(progress);
                                envelopes_progresses.insert(folder, progress.clone());
                            }
                        }
                        SyncEvent::ProcessedEmailHunk(hunk) => {
                            main_progress.inc(1);
                            let mut progresses = sub_progresses.lock().unwrap();
                            if let Some(progress) = progresses.get_mut(hunk.folder()) {
                                progress.inc(1);
                                if progress.position() == (progress.length().unwrap() - 1) {
                                    progress.set_style(SUB_PROGRESS_DONE_STYLE.clone())
                                } else {
                                    progress.set_message(format!("{hunk}…"));
                                }
                            }
                        }
                        SyncEvent::ProcessedAllEmailHunks => {
                            let mut progresses = sub_progresses.lock().unwrap();
                            for progress in progresses.values() {
                                progress.finish_and_clear()
                            }
                            progresses.clear();

                            main_progress.set_length(100);
                            main_progress.set_position(100);
                            main_progress.set_message("Expunging folders…");
                        }
                        SyncEvent::ExpungedAllFolders => {
                            main_progress.finish_and_clear();
                        }
                        _ => {
                            main_progress.tick();
                        }
                    };

                    async { Ok(()) }
                })
                .sync()
                .await?;

            let folders_patch_err = report
                .folder
                .patch
                .iter()
                .filter_map(|(hunk, err)| err.as_ref().map(|err| (hunk, err)))
                .collect::<Vec<_>>();
            if !folders_patch_err.is_empty() {
                printer.print_log("")?;
                printer.print_log("Errors occurred while applying the folders patch:")?;
                folders_patch_err
                    .iter()
                    .try_for_each(|(hunk, err)| printer.print_log(format!(" - {hunk}: {err}")))?;
            }

            let envelopes_patch_err = report
                .email
                .patch
                .iter()
                .filter_map(|(hunk, err)| err.as_ref().map(|err| (hunk, err)))
                .collect::<Vec<_>>();
            if !envelopes_patch_err.is_empty() {
                printer.print_log("")?;
                printer.print_log("Errors occurred while applying the envelopes patch:")?;
                for (hunk, err) in envelopes_patch_err {
                    printer.print_log(format!(" - {hunk}: {err}"))?;
                }
            }

            printer.print(format!("Account {account_name} successfully synchronized!"))?;
        }

        Ok(())
    }
}
