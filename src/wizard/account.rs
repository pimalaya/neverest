//! Account-creation wizard.
//!
//! Placeholder for the new pimalaya-cli + io-discovery flow. The
//! production wizard will prompt for an account name, then walk
//! through left + right backend selection (IMAP / JMAP / Maildir),
//! seed IMAP/JMAP defaults via [`io_discovery::autoconfig`], and
//! persist the [`Config`] via [`Config::write`].

use std::path::Path;

use anyhow::{Result, bail};

use crate::config::Config;

pub fn run_or_exit(_target: &Path) -> Result<Config> {
    bail!(
        "no config file found and the wizard is not yet wired against \
         pimalaya-cli; create one manually and re-run, or wait for the \
         wizard rewrite"
    )
}
