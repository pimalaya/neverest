//! # Configuration
//!
//! Module dedicated to the main configuration of Neverest CLI.

#[cfg(feature = "wizard")]
pub mod wizard;

use color_eyre::eyre::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

use pimalaya_tui::config::TomlConfig;

use crate::account::config::AccountConfig;

/// The main configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// The configuration of all the accounts.
    pub accounts: HashMap<String, AccountConfig>,
}

impl TomlConfig<AccountConfig> for Config {
    fn project_name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }
}

impl Config {
    /// Create and save a TOML configuration using the wizard.
    ///
    /// If the user accepts the confirmation, the wizard starts and
    /// help him to create his configuration file. Otherwise the
    /// program stops.
    ///
    /// NOTE: the wizard can only be used with interactive shells.
    #[cfg(feature = "wizard")]
    async fn from_wizard(path: &PathBuf) -> Result<Self> {
        Self::confirm_from_wizard(path)?;
        wizard::configure(path).await
    }

    /// Read and parse the TOML configuration from default paths.
    pub async fn from_default_paths() -> Result<Self> {
        match Self::first_valid_default_path() {
            Some(path) => Self::from_paths(&[path]),
            #[cfg(feature = "wizard")]
            None => Self::from_wizard(&Self::default_path()?).await,
            #[cfg(not(feature = "wizard"))]
            None => color_eyre::eyre::bail!("cannot find config file from default paths"),
        }
    }

    /// Read and parse the TOML configuration at the optional given
    /// path.
    ///
    /// If the given path exists, then read and parse the TOML
    /// configuration from it.
    ///
    /// If the given path does not exist, then create it using the
    /// wizard.
    ///
    /// If no path is given, then either read and parse the TOML
    /// configuration at the first valid default path, otherwise
    /// create it using the wizard.  wizard.
    pub async fn from_paths_or_default(paths: &[PathBuf]) -> Result<Self> {
        match paths.len() {
            0 => Self::from_default_paths().await,
            _ if paths[0].exists() => Self::from_paths(paths),
            #[cfg(feature = "wizard")]
            _ => Self::from_wizard(&paths[0]).await,
            #[cfg(not(feature = "wizard"))]
            _ => color_eyre::eyre::bail!("cannot find config file from default paths"),
        }
    }

    pub fn into_account_config(
        &self,
        account_name: Option<&str>,
    ) -> Result<(String, AccountConfig)> {
        #[allow(unused_mut)]
        let (account_name, mut account_config) = match account_name {
            Some("default") | Some("") | None => self
                .accounts
                .iter()
                .find_map(|(name, config)| {
                    config
                        .default
                        .filter(|default| *default)
                        .map(|_| (name.to_owned(), config.clone()))
                })
                .ok_or_else(|| anyhow!("cannot find default account")),
            Some(name) => self
                .accounts
                .get(name)
                .map(|config| (name.to_owned(), config.clone()))
                .ok_or_else(|| anyhow!("cannot find account {name}")),
        }?;

        account_config.configure(account_name.as_str())?;

        Ok((account_name, account_config))
    }
}
