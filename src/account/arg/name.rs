//! # Account name argument
//!
//! Module dedicated to the account name CLI argument.

use clap::Parser;

/// The optional account name argument parser.
#[derive(Debug, Parser)]
pub struct OptionalAccountNameArg {
    /// The name of the account.
    ///
    /// The account name corresponds to the name of the TOML table
    /// entry at path `accounts.<name>`. If omitted, the account
    /// marked as default will be used.
    #[arg(name = "account_name", value_name = "ACCOUNT")]
    pub name: Option<String>,
}
