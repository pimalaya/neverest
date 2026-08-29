//! # Check command
//!
//! Reports what the store keeps, then opens every source and lists its
//! collections, surfacing credential, network or config errors before a real
//! sync.

use std::{fmt, path::PathBuf};

use anyhow::{Result, bail};
use clap::Parser;
use log::info;
use pimalaya_cli::{printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{account::Account, client, config::Config};

/// Probes every configured source before a real sync.
///
/// Reports the account's namespaces, then opens each source and lists its
/// collections, surfacing credential, network or config errors before a real
/// sync.
#[derive(Debug, Parser)]
pub struct CheckCommand {}

impl CheckCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        info!("checking account {name}");

        let mode = account_config.mode()?.to_string();

        // Every credential at once, so a check costs one unlock per password
        // command rather than one per endpoint.
        let account = Account::resolve(&account_config)?;

        let mut sources = Vec::new();

        for endpoint in account_config.endpoints()?.keys() {
            sources.push(check_source(endpoint, &account)?);
        }

        printer.out(CheckOutput {
            account: name,
            mode,
            sources,
        })
    }
}

/// Opens the source and probes it with a `list_collections` call.
fn check_source(label: &str, account: &Account) -> Result<SourceCheck> {
    let s = Spinner::start(format!("Checking source {label}…"));
    let mut client = client::open(&account.get(label)?)?;
    let collections = client.list_collections(false)?.len();
    s.success(format!(
        "Checked source {label} ({collections} collections)"
    ));

    Ok(SourceCheck {
        source: label.to_owned(),
        collections,
    })
}

/// What `neverest check` reports: the account's declared mode, and every
/// source that answered.
///
/// A source that did not answer is not in here: the command stops on the
/// first failure, so reaching this output means every endpoint opened.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckOutput {
    /// The account that was checked.
    pub account: String,
    /// What the account does, as its mode reads.
    pub mode: String,
    /// One entry per endpoint the account opens, source and target alike.
    pub sources: Vec<SourceCheck>,
}

impl fmt::Display for CheckOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{mode}", mode = self.mode)?;
        writeln!(f)?;

        for source in &self.sources {
            writeln!(f, " - {source}")?;
        }

        writeln!(f)?;
        writeln!(f, "Account {account} looks healthy", account = self.account)
    }
}

/// One endpoint that answered, and how much it holds.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SourceCheck {
    /// The endpoint's pimdir source id.
    pub source: String,
    /// How many collections it listed.
    pub collections: usize,
}

impl fmt::Display for SourceCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            source,
            collections,
        } = self;
        write!(f, "{source} ({collections} collection(s))")
    }
}
