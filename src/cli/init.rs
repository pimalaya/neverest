//! `neverest init` command.
//!
//! Bootstraps an account's per-side state: probes the remote
//! connection(s) so IMAP CAPABILITY / JMAP session GET surfaces any
//! credential / network problem up front, creates the m2dir root and
//! marker on local sides, then writes an empty cache snapshot at
//! `$XDG_CACHE_HOME/neverest/<account>/state.json`. The cache file's
//! presence is the single source of truth for "this account is
//! initialized"; [`crate::account::sync`] refuses to run when it is
//! missing, and this command refuses to run when it is present.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{
    config::Config,
    side::Side,
    sync::cache::{CacheSnapshot, cache_path},
};

/// Initialize an account's per-side state.
///
/// Probes both sides (open the remote connection, create the m2dir
/// store root) and writes an empty cache snapshot that subsequent
/// `sync` runs consume. Refuses to run if the account is already
/// initialized.
#[derive(Debug, Parser)]
pub struct InitCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl InitCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        let cache = cache_path(&name)?;
        if cache.exists() {
            let p = cache.display();
            bail!("Account `{name}` already initialized, delete `{p}` to reset");
        }

        // Open and immediately drop each side: the connection handshake is the
        // probe (CAPABILITY for IMAP, session GET for JMAP), the m2dir branch
        // creates the store root and marker via `init_side`. `sync` will reopen
        // on its own.
        let s = Spinner::start("Initializing left side…");
        Side::Left
            .init(account_config.left.clone())
            .context("Initialize left side")?;
        s.success("Initialized left side");

        let s = Spinner::start("Initializing right side…");
        Side::Right
            .init(account_config.right.clone())
            .context("Initialize right side")?;
        s.success("Initialized right side");

        let s = Spinner::start("Writing initial cache snapshot…");
        CacheSnapshot::default()
            .save(&cache)
            .context(format!("Write initial cache `{}`", cache.display()))?;
        s.success("Wrote initial cache snapshot");

        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized"
        )))
    }
}
