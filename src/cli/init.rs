//! `neverest init` command: probes both sides and writes the initial
//! state snapshot that subsequent sync runs consume.
//!
//! The state file's presence is the single source of truth for "this
//! account is initialized".

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{client, config::Config, sync::state::StateSnapshot};

/// Initializes an account's per-side state; refuses to run if it is
/// already initialized.
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

        let state = StateSnapshot::path(&name)?;
        if state.exists() {
            let p = state.display();
            bail!("Account `{name}` already initialized, delete `{p}` to reset");
        }

        let s = Spinner::start("Initializing left side…");
        client::init(account_config.left.clone()).context("Initialize left side")?;
        s.success("Initialized left side");

        let s = Spinner::start("Initializing right side…");
        client::init(account_config.right.clone()).context("Initialize right side")?;
        s.success("Initialized right side");

        let s = Spinner::start("Writing initial state snapshot…");
        StateSnapshot::default()
            .save(&state)
            .context(format!("Write initial state `{}`", state.display()))?;
        s.success("Wrote initial state snapshot");

        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized"
        )))
    }
}
