//! # Configuration
//!
//! Module dedicated to the main configuration of Neverest CLI.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{account::config::TomlAccountConfig, backend::config::BackendConfig};

/// The main configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TomlConfig {
    /// The configuration of all the accounts.
    pub accounts: HashMap<String, TomlAccountConfig>,
}

#[async_trait]
impl pimalaya_tui::terminal::config::TomlConfig for TomlConfig {
    type TomlAccountConfig = TomlAccountConfig;

    fn project_name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn get_default_account_config(&self) -> Option<(String, Self::TomlAccountConfig)> {
        self.accounts.iter().find_map(|(name, account)| {
            account
                .default
                .filter(|default| *default)
                .map(|_| (name.to_owned(), account.clone()))
        })
    }

    fn get_account_config(&self, name: &str) -> Option<(String, Self::TomlAccountConfig)> {
        self.accounts
            .get(name)
            .map(|account| (name.to_owned(), account.clone()))
    }

    #[cfg(feature = "wizard")]
    async fn from_wizard(path: &std::path::Path) -> color_eyre::Result<Self> {
        use std::process::exit;

        use pimalaya_tui::terminal::{print, prompt};

        use crate::account;

        print::warn(format!("Cannot find configuration at {}.", path.display()));

        if !prompt::bool("Would you like to create one with the wizard?", true)? {
            exit(0);
        }

        print::section("Configuring your default account");

        let mut config = TomlConfig::default();

        let (account_name, account_config) = account::wizard::configure().await?;
        config.accounts.insert(account_name, account_config);
        config.write(path)?;

        Ok(config)
    }

    fn to_toml_account_config(
        &self,
        account_name: Option<&str>,
    ) -> pimalaya_tui::Result<(String, Self::TomlAccountConfig)> {
        #[allow(unused_mut)]
        let (name, mut config) = match account_name {
            Some("default") | Some("") | None => self
                .get_default_account_config()
                .ok_or(pimalaya_tui::Error::GetDefaultAccountConfigError),
            Some(name) => self
                .get_account_config(name)
                .ok_or_else(|| pimalaya_tui::Error::GetAccountConfigError(name.to_owned())),
        }?;

        #[cfg(all(feature = "imap", feature = "keyring"))]
        if let BackendConfig::Imap(imap_config) = &mut config.left.backend {
            imap_config.auth.replace_empty_secrets(&name)?;
        }

        #[cfg(all(feature = "imap", feature = "keyring"))]
        if let BackendConfig::Imap(imap_config) = &mut config.right.backend {
            imap_config.auth.replace_empty_secrets(&name)?;
        }

        Ok((name, config))
    }
}
