//! # Check up account command
//!
//! This module contains the [`clap`] command for checking up left and
//! right backends integrity of a given account.

use anyhow::Result;
use clap::Parser;
use email::backend::{Backend, BackendBuilder};
#[cfg(feature = "imap")]
use email::imap::{ImapContextBuilder, ImapContextSync};
#[cfg(feature = "maildir")]
use email::maildir::{MaildirContextBuilder, MaildirContextSync};
#[cfg(feature = "notmuch")]
use email::notmuch::{NotmuchContextBuilder, NotmuchContextSync};
use log::info;
use std::sync::Arc;

use crate::{
    account::arg::name::OptionalAccountNameArg, backend::config::BackendConfig, config::Config,
    printer::Printer,
};

/// Check up the given account.
///
/// This command performs a checkup of the given account. It checks if
/// the configuration is valid, if backend can be created and if
/// sessions work as expected.
#[derive(Debug, Parser)]
pub struct CheckUpAccountCommand {
    #[command(flatten)]
    pub account: OptionalAccountNameArg,
}

impl CheckUpAccountCommand {
    pub async fn execute(self, printer: &mut impl Printer, config: &Config) -> Result<()> {
        info!("executing check up account command");

        let (name, config) = config.into_account_config(self.account.name.as_deref())?;
        printer.print_log(format!("Checking `{name}` account integrity…"))?;

        let folder_filter = config.folder.map(|c| c.filter).unwrap_or_default();
        let envelope_filter = config.envelope.map(|c| c.filter).unwrap_or_default();

        let (left_backend, left_config) = config.left.into_account_config(
            name.clone(),
            folder_filter.clone(),
            envelope_filter.clone(),
        );

        match left_backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(imap_config) => {
                printer.print_log("Checking left IMAP integrity…")?;
                let ctx = ImapContextBuilder::new(left_config.clone(), Arc::new(imap_config));
                BackendBuilder::new(left_config.clone(), ctx)
                    .check_up::<Backend<ImapContextSync>>()
                    .await?;
            }
            #[cfg(feature = "maildir")]
            BackendConfig::Maildir(maildir_config) => {
                printer.print_log("Checking left Maildir integrity…")?;
                let ctx = MaildirContextBuilder::new(left_config.clone(), Arc::new(maildir_config));
                BackendBuilder::new(left_config.clone(), ctx)
                    .check_up::<Backend<MaildirContextSync>>()
                    .await?;
            }
            #[cfg(feature = "notmuch")]
            BackendConfig::Notmuch(notmuch_config) => {
                printer.print_log("Checking left Notmuch integrity…")?;
                let ctx = NotmuchContextBuilder::new(left_config.clone(), Arc::new(notmuch_config));
                BackendBuilder::new(left_config.clone(), ctx)
                    .check_up::<Backend<NotmuchContextSync>>()
                    .await?;
            }
        };

        let (right_backend, right_config) =
            config
                .right
                .into_account_config(name.clone(), folder_filter, envelope_filter);

        match right_backend {
            #[cfg(feature = "imap")]
            BackendConfig::Imap(imap_config) => {
                printer.print_log("Checking right IMAP integrity…")?;
                let ctx = ImapContextBuilder::new(right_config.clone(), Arc::new(imap_config));
                BackendBuilder::new(right_config.clone(), ctx)
                    .check_up::<Backend<ImapContextSync>>()
                    .await?;
            }
            #[cfg(feature = "maildir")]
            BackendConfig::Maildir(maildir_config) => {
                printer.print_log("Checking right Maildir integrity…")?;
                let ctx =
                    MaildirContextBuilder::new(right_config.clone(), Arc::new(maildir_config));
                BackendBuilder::new(right_config.clone(), ctx)
                    .check_up::<Backend<MaildirContextSync>>()
                    .await?;
            }
            #[cfg(feature = "notmuch")]
            BackendConfig::Notmuch(notmuch_config) => {
                printer.print_log("Checking right Notmuch integrity…")?;
                let ctx =
                    NotmuchContextBuilder::new(right_config.clone(), Arc::new(notmuch_config));
                BackendBuilder::new(right_config.clone(), ctx)
                    .check_up::<Backend<NotmuchContextSync>>()
                    .await?;
            }
        };

        printer.print("Checkup successfully completed!")
    }
}
