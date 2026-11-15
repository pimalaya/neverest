//! # Account commands
//!
//! The `account` group: listing the accounts the configuration declares,
//! and checking that what each one stands on answers.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use pimalaya_cli::{
    account::{AccountListOutput, AccountRow},
    printer::Printer,
};

use crate::{
    cli::check::CheckCommand,
    config::{self, AccountConfig, Config},
};

/// Inspect the accounts the configuration declares.
///
/// An account is a named group of sources over one pimdir store, written
/// by hand from the sample; these subcommands only read it. Like every
/// command, they run over the accounts `-a` names, or every one.
#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    List(AccountListCommand),
    Check(CheckCommand),
}

impl AccountCommand {
    /// Runs the subcommand over the accounts `-a` names, or every one.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<()> {
        match self {
            Self::List(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Check(cmd) => cmd.execute(printer, config_paths, accounts),
        }
    }
}

/// List the accounts the configuration declares.
///
/// One line per account, sorted by name: the backend tables it writes (its
/// sources' and targets', `smtp` for a send channel), those this build
/// leaves out marked `(not built)`, and `default`, which every account is:
/// a command naming no `-a` runs over them all. Nothing is opened, which
/// `account check` does.
///
/// JSON output: `{"accounts": [{"name", "default", "backends",
/// "notBuilt"}]}`, pimalaya-cli's account row.
#[derive(Debug, Parser)]
pub struct AccountListCommand;

impl AccountListCommand {
    /// Loads the configuration and prints the accounts `accounts` names,
    /// every one when it names none.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<()> {
        let config = Config::load(config_paths)?;
        let accounts = config
            .select(accounts)?
            .into_iter()
            .map(|(name, account)| row(name, &account))
            .collect();

        printer.out(AccountListOutput { accounts })
    }
}

/// The row of the account `name`, its backends split by whether this build
/// serves them.
fn row(name: String, account: &AccountConfig) -> AccountRow {
    let (backends, not_built) = account
        .backends()
        .into_iter()
        .partition(|backend| config::is_built(backend));

    AccountRow {
        name,
        default: true,
        backends,
        not_built,
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::account::*;

    /// An account lists every backend table it writes, its sources' and
    /// targets', once each, `smtp` for its send channel, and is a default
    /// one: a command naming no `-a` runs over it.
    #[test]
    fn an_account_lists_the_backends_its_endpoints_write() {
        let account: AccountConfig = toml::from_str(
            "imap.server = \"imaps://imap.example.org\"\n\
             smtp.server = \"smtps://smtp.example.org\"\n\
             sources.other.imap.server = \"imaps://imap.other.org\"\n\
             one-way = true\n\
             targets.cal.caldav.server = \"https://dav.example.org\"\n\
             targets.cal.caldav.auth = \"none\"\n\
             jmap.server = \"https://jmap.example.org\"\n\
             jmap.auth.bearer.token.raw = \"token\"\n",
        )
        .unwrap();

        let row = row(String::from("work"), &account);

        let mut every: Vec<&str> = row.backends.iter().chain(&row.not_built).copied().collect();
        every.sort_by_key(|name| config::BACKENDS.iter().position(|known| known == name));
        assert_eq!(every, ["imap", "caldav", "jmap", "smtp"]);
        assert!(row.not_built.contains(&"jmap"), "no build opens JMAP");
        assert_eq!(row.backends.contains(&"imap"), cfg!(feature = "imap"));
        assert!(row.default);
    }
}
