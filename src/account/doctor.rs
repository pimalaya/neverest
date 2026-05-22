//! `neverest doctor` command.
//!
//! Opens both sides and asks each one to `list_mailboxes`. The
//! operation itself is cheap; the value is in surfacing the
//! credential / network / config errors that would otherwise only
//! show up during a real sync.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pimalaya_cli::printer::Printer;

use crate::cli::load_or_wizard;
use crate::side::{Side, SideClient, take_account};

#[derive(Debug, Parser)]
pub struct DoctorAccountCommand {
    #[arg(value_name = "ACCOUNT")]
    pub account: Option<String>,
}

impl DoctorAccountCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let config = load_or_wizard(config_paths)?;
        let (name, account, account_config) = take_account(config, self.account.as_deref())?;

        log_line(format!("Checking account `{name}`\u{2026}\n"))?;

        check_side(
            "left",
            account_config.left.clone(),
            account.clone(),
            Side::Left,
            printer,
        )?;
        check_side("right", account_config.right, account, Side::Right, printer)?;

        printer.out(format!("Account {name} looks healthy."))?;
        Ok(())
    }
}

fn log_line(msg: impl AsRef<str>) -> Result<()> {
    eprint!("{}", msg.as_ref());
    Ok(())
}

fn check_side(
    label: &str,
    side_config: crate::config::SideConfig,
    account: crate::account::context::Account,
    side: Side,
    _printer: &mut impl Printer,
) -> Result<()> {
    log_line(format!("- {label}: opening client\u{2026}\n"))?;
    let client = SideClient::open(side_config, account, side)?;
    let mut email = client.into_email_client();
    let mailboxes = email.list_mailboxes(false)?;
    log_line(format!(
        "- {label}: OK ({n} mailboxes)\n",
        n = mailboxes.len()
    ))?;
    Ok(())
}
